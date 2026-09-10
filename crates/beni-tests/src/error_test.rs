use crate::support::open_mrb;
use beni::{Error, Mrb};

#[test]
fn new_builds_an_exception_error_carrying_the_message() {
    let mrb = open_mrb();
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is a core class");

    let err = Error::new(&mrb, runtime_error, "boom");

    // The constructor produces the Exception variant, and the
    // exception renders the message it was built with.
    assert!(matches!(err, Error::Exception(_)));
    assert_eq!(err.message(&mrb), "boom");
}

#[test]
fn argnum_renders_the_fixed_count_form() {
    let mrb = open_mrb();

    // min == max: "wrong number of arguments (given 3, expected 2)".
    let err = Error::argnum(&mrb, 3, 2, 2);

    assert!(matches!(err, Error::Exception(_)));
    let exc = match err {
        Error::Exception(v) => v,
        other => unreachable!("argnum must surface as Error::Exception, got {other}"),
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
    let message = Error::Exception(exc).message(&mrb);
    assert!(
        message.contains("given 3") && message.contains("expected 2"),
        "unexpected message: {message}"
    );
}

#[test]
fn argnum_renders_the_open_ended_form_for_a_negative_max() {
    let mrb = open_mrb();

    // max < 0: "expected 2+" — at least `min`, no upper bound.
    let err = Error::argnum(&mrb, 1, 2, -1);

    let message = err.message(&mrb);
    assert!(
        message.contains("given 1") && message.contains("expected 2+"),
        "unexpected message: {message}"
    );
}

#[test]
fn argnum_renders_the_range_form_for_distinct_bounds() {
    let mrb = open_mrb();

    // min < max: "expected 2..4" — an inclusive range.
    let err = Error::argnum(&mrb, 5, 2, 4);

    let message = err.message(&mrb);
    assert!(
        message.contains("given 5") && message.contains("expected 2..4"),
        "unexpected message: {message}"
    );
}

#[test]
fn backtrace_reads_the_frames_a_raise_under_a_context_carries() {
    use beni::Ccontext;

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"backtrace_test.rb")
        .expect("allocating the compile context must succeed");

    let err = cxt
        .load_nstring(b"def outer\n  inner\nend\ndef inner\n  raise 'deep'\nend\nouter\n")
        .expect_err("the raise must surface as Err");

    let frames = err.backtrace(&mrb);
    assert!(!frames.is_empty(), "a stamped filename packs a backtrace");
    assert!(
        frames.iter().any(|f| f.contains("backtrace_test.rb")),
        "the frames name the stamped filename: {frames:?}"
    );
    assert!(
        frames.iter().any(|f| f.contains("inner")),
        "the frames name the raising method: {frames:?}"
    );
}

#[test]
fn backtrace_answers_empty_for_an_error_carrying_no_exception() {
    use beni::Ccontext;

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"backtrace_test.rb")
        .expect("allocating the compile context must succeed");

    let syntax = cxt
        .load_nstring(b"end")
        .expect_err("source that does not parse must surface as Err");
    assert!(
        syntax.backtrace(&mrb).is_empty(),
        "a program that never compiled never ran"
    );

    assert!(Error::Panic("boom".to_owned()).backtrace(&mrb).is_empty());
}

#[test]
fn backtrace_answers_empty_for_an_exception_holding_none() {
    let mrb = open_mrb();

    // Built in Rust rather than raised, so nothing ever packed frames
    // onto it.
    let err = Error::new(&mrb, mrb.class_get(c"RuntimeError").unwrap(), "unraised");

    assert!(err.backtrace(&mrb).is_empty());
}

/// Collect with nothing but `err` holding its exception, and require
/// that exception to still be one. A reclaimed object reads back as no
/// longer an exception, so this observes survival without dereferencing
/// what the collector may have freed.
fn assert_exception_survives_a_collection(mrb: &Mrb, err: &Error) {
    let Error::Exception(exc) = err else {
        panic!("the error must carry an exception, got {err:?}")
    };
    mrb.full_gc();
    assert!(exc.is_exception(), "the collection reclaimed the exception");
}

#[test]
fn a_load_error_keeps_its_exception_through_a_collection() {
    let mrb = open_mrb();

    let err = mrb
        .load_string(b"raise 'boom'")
        .expect_err("a raising script must come back Err");

    assert_exception_survives_a_collection(&mrb, &err);
    assert!(err.message(&mrb).contains("boom"));
}

#[test]
fn a_context_load_error_keeps_its_frames_through_a_collection() {
    use beni::Ccontext;

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"collected.rb").expect("allocating the context must succeed");

    let err = cxt
        .load_nstring(b"raise 'boom'")
        .expect_err("a raising script must come back Err");

    assert_exception_survives_a_collection(&mrb, &err);
    // The frames are packed onto the exception and unpacked on demand,
    // so they are only readable while the exception itself is.
    assert!(
        err.backtrace(&mrb)
            .iter()
            .any(|frame| frame.contains("collected.rb")),
        "the stamped frames outlive the collection too"
    );
}

#[test]
fn a_bytecode_load_error_keeps_its_exception_through_a_collection() {
    use beni::{Ccontext, DumpOptions};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"collected.rb").expect("allocating the context must succeed");
    let bytes = cxt
        .compile(b"raise 'boom'")
        .expect("the source must compile")
        .dump(&mrb, DumpOptions::default())
        .expect("a Proc compiled from source must dump");

    let err = mrb
        .load_bytecode(&bytes)
        .expect_err("a program that raises must not come back Ok");

    assert_exception_survives_a_collection(&mrb, &err);
    assert!(err.message(&mrb).contains("boom"));
}

#[test]
fn a_protected_raise_keeps_its_exception_through_a_collection() {
    let mrb = open_mrb();

    let err = mrb
        .protect(|m| {
            // SAFETY: `m` is the live VM inside the protected frame;
            // `RuntimeError` is a core class so the lookup cannot fail;
            // `mrb_raise` long-jumps to the protect frame.
            unsafe {
                let runtime_error = beni::sys::mrb_class_get(m.as_ptr(), c"RuntimeError".as_ptr());
                beni::sys::mrb_raise(m.as_ptr(), runtime_error, c"boom".as_ptr());
            }
        })
        .expect_err("a raise inside the body must surface as Err");

    assert_exception_survives_a_collection(&mrb, &err);
    assert!(err.message(&mrb).contains("boom"));
}
