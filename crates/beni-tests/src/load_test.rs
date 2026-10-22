use crate::support::open_mrb;
use crate::support::OwnedBytes;
use beni::prelude::*;
use beni::{Ccontext, DumpOptions, Error, FromValue, Module, Mrb, RString, Value};

const HEADER_LEN: usize = core::mem::size_of::<beni::sys::rite_binary_header>();

#[test]
fn load_string_evaluates_a_valid_expression_to_its_value() {
    let mrb = open_mrb();

    let got = mrb
        .eval::<beni::Value>("1 + 2")
        .expect("a valid expression must come back Ok");

    assert_eq!(i32::from_value(got), Some(3));
    assert!(
        mrb.pending_exc().is_nil(),
        "a successful eval must leave no pending exception"
    );
}

#[test]
fn load_string_surfaces_a_raising_script_as_err() {
    let mrb = open_mrb();

    let err = mrb
        .eval::<beni::Value>("raise 'kaboom'")
        .expect_err("a raising script must come back Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("kaboom")),
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "the pending exception must be cleared as the Err crosses out"
    );
}

#[test]
fn load_string_surfaces_a_parse_failure_with_its_location() {
    let mrb = open_mrb();

    let err = mrb
        .eval::<beni::Value>("a = 1\nb = 2\nend\n")
        .expect_err("unparseable source must come back Err");

    match err {
        Error::Syntax(parse) => {
            assert_eq!(
                parse.line(),
                3,
                "a borrowed context carries the location a caller's context does"
            );
            assert!(!parse.message().is_empty());
        }
        other => panic!("a parse failure must surface as Error::Syntax, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "a program that never compiled leaves no exception on the handle"
    );
}

#[test]
fn load_string_stamps_no_filename_so_a_raise_carries_no_backtrace() {
    let mrb = open_mrb();

    let err = mrb
        .eval::<beni::Value>("def outer\n  raise 'deep'\nend\nouter\n")
        .expect_err("a raising script must come back Err");

    assert!(
        err.backtrace(&mrb).is_empty(),
        "the borrowed context is unnamed, so nothing packs frames"
    );
}

#[test]
fn load_string_still_runs_source_the_compiler_warns_about() {
    let mrb = open_mrb();

    // The borrowed context captures the warning and is released with
    // it; a caller who wants the warning holds a context of their own.
    let got = mrb
        .eval::<beni::Value>("begin\n  1\nelse\n  2\nend\n")
        .expect("a warning must not change the load's outcome");

    assert_eq!(i32::from_value(got), Some(2));
}

/// The `ScriptError` a blob mruby cannot read as a program comes back
/// as, rendered. Asserts the class as well as the failure, so a load
/// that reported the right words under the wrong class still fails
/// here — the class is what a Ruby-side bare `rescue` reads.
fn structural_failure(mrb: &Mrb, blob: &[u8]) -> String {
    // SAFETY: each blob here fails structural validation, which the
    // read rejects before anything runs.
    let err =
        unsafe { mrb.load_irep(blob) }.expect_err("a blob that is not a program must not load");
    let Error::Exception(exc) = &err else {
        panic!("a structural failure carries an exception, got {err:?}");
    };
    assert_eq!(
        exc.classname(mrb),
        "ScriptError",
        "the class mruby's own irep loader reports the same condition under"
    );
    assert!(
        mrb.pending_exc().is_nil(),
        "the error carries the exception, so nothing stays pending"
    );
    err.message(mrb)
}

#[test]
fn load_irep_classifies_a_blob_shorter_than_the_header() {
    let mrb = open_mrb();

    assert!(structural_failure(&mrb, b"RITE").contains("shorter than RITE binary header"));
}

#[test]
fn load_irep_classifies_a_non_rite_ident() {
    let mrb = open_mrb();

    assert!(structural_failure(&mrb, &[b'X'; HEADER_LEN]).contains("not RITE format"));
}

#[test]
fn load_irep_classifies_a_rite_version_mismatch() {
    let mrb = open_mrb();
    let mut blob = [0u8; HEADER_LEN];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(b"0000");

    assert!(structural_failure(&mrb, &blob).contains("RITE version mismatch"));
}

#[test]
fn load_irep_classifies_a_corrupt_body() {
    let mrb = open_mrb();
    let mut blob = [0u8; HEADER_LEN + 8];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(&beni::sys::RITE_BINARY_FORMAT_VER[..4]);

    assert!(structural_failure(&mrb, &blob).contains("failed structural validation"));
}

#[test]
fn load_irep_leaves_the_vm_usable_after_a_structural_failure() {
    let mrb = open_mrb();

    let _ = structural_failure(&mrb, b"not RITE bytecode");

    let alive = mrb
        .eval::<beni::Value>("1 + 1")
        .expect("the VM survives a load that never became a program");
    assert_eq!(i32::from_value(alive), Some(2));
}

#[test]
fn load_irep_hands_back_an_exception_the_program_raised() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, b"raise ArgumentError, 'from the loaded program'");

    // SAFETY: the blob is this interpreter's own dump.
    let err =
        unsafe { mrb.load_irep(&bytes) }.expect_err("a program that raises must not come back Ok");

    let Error::Exception(exc) = &err else {
        panic!("a raise carries an exception, got {err:?}");
    };
    assert_eq!(
        exc.classname(&mrb),
        "ArgumentError",
        "the program's own exception, not the loader's ScriptError"
    );
    assert!(
        mrb.pending_exc().is_nil(),
        "the error carries the exception, so nothing stays pending"
    );
    assert!(err.message(&mrb).contains("from the loaded program"));
}

fn dumped(mrb: &Mrb, source: &[u8]) -> Vec<u8> {
    Ccontext::new(mrb, c"dumped.rb")
        .expect("allocating the context must succeed")
        .compile(source)
        .expect("the source must compile")
        .dump(mrb, DumpOptions::default())
        .expect("a Proc compiled from source must dump")
}

#[test]
fn read_irep_yields_a_program_that_runs_only_when_called() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, b"$runs = ($runs || 0) + 1; 6 * 7");
    let runs = mrb.intern_cstr(c"$runs").expect("the name interns");

    // SAFETY: the blob is this interpreter's own dump.
    let program = unsafe { mrb.read_irep(&bytes) }.expect("the dump must read back");
    assert!(
        mrb.gv_get(runs).is_nil(),
        "reading must not run the program"
    );

    let got = program
        .call(&mrb, &[])
        .unwrap_or_else(|_| panic!("the program must run when called"));
    assert_eq!(i64::from_value(got), Some(42));
    assert_eq!(i64::from_value(mrb.gv_get(runs)), Some(1));
}

#[test]
fn read_irep_and_load_irep_answer_the_same_value_for_a_top_level_return() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, b"return 5\n6");

    // SAFETY: the blob is this interpreter's own dump.
    let called = unsafe { mrb.read_irep(&bytes) }
        .expect("the dump must read back")
        .call(&mrb, &[])
        .unwrap_or_else(|_| panic!("the program must run when called"));
    // SAFETY: as above.
    let loaded = unsafe { mrb.load_irep(&bytes) }.expect("the dump must load");

    assert_eq!(i64::from_value(called), Some(5));
    assert_eq!(i64::from_value(loaded), Some(5));
}

#[test]
fn read_irep_refuses_an_unreadable_blob_as_a_script_error() {
    let mrb = open_mrb();

    // SAFETY: the blob fails structural validation, which the read
    // rejects before anything runs.
    let err = unsafe { mrb.read_irep(b"RITE") }
        .err()
        .expect("a blob that is not a program must not read");

    let Error::Exception(exc) = &err else {
        panic!("a structural failure carries an exception, got {err:?}");
    };
    assert_eq!(exc.classname(&mrb), "ScriptError");
    assert!(err
        .message(&mrb)
        .contains("shorter than RITE binary header"));
}

#[test]
fn read_irep_leaves_a_script_error_the_program_raises_to_its_call() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, b"raise ScriptError, 'from the program'");

    // SAFETY: the blob is this interpreter's own dump.
    let program =
        unsafe { mrb.read_irep(&bytes) }.expect("a program that raises when run still reads");
    let err = program
        .call(&mrb, &[])
        .expect_err("the raise must come back from the call");

    let Error::Exception(exc) = &err else {
        panic!("a raise carries an exception, got {err:?}");
    };
    assert_eq!(exc.classname(&mrb), "ScriptError");
    assert!(err.message(&mrb).contains("from the program"));
}

// A registered method answers the exception the load inside it came
// back with, or nil when it came back with none — so a raise that
// escaped the load reaches the method's caller instead.
fn caught(outcome: Result<Value, Error>) -> Value {
    match outcome {
        Err(Error::Exception(exc)) => exc,
        _ => beni::value::qnil().as_value(),
    }
}

fn load_string_inside(mrb: &Mrb, _self: Value) -> Value {
    caught(mrb.eval::<beni::Value>("raise 'from the nested load'"))
}

fn load_irep_inside(mrb: &Mrb, _self: Value, blob: RString) -> Value {
    // SAFETY: the blob is this interpreter's own dump.
    caught(unsafe { mrb.load_irep(&blob.owned_bytes()) })
}

// A program defining a constant and a method, so where each landed
// shows whether it ran at the top level.
const TOP_LEVEL_PROGRAM: &[u8] =
    b"TOP_LEVEL_ANSWER = 42\ndef top_level_answer; TOP_LEVEL_ANSWER; end";

fn load_irep_in_loader(mrb: &Mrb, _self: Value, blob: RString) -> Value {
    // SAFETY: the blob is this interpreter's own dump.
    unsafe { mrb.load_irep(&blob.owned_bytes()) }.expect("the program must run")
}

#[test]
fn load_irep_inside_a_method_of_another_class_runs_at_the_top_level() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, TOP_LEVEL_PROGRAM);
    let loader = mrb
        .define_class(c"BeniBytecodeLoader", mrb.class_object())
        .expect("defining the class must succeed");
    loader
        .define_method(&mrb, c"run", beni::method!(load_irep_in_loader, 1))
        .expect("defining the method must succeed");

    loader
        .new_instance(&mrb, &[])
        .expect("instantiating must succeed")
        .funcall(&mrb, c"run", &[mrb.str_new(&bytes).as_value()])
        .expect("the load must come back Ok");

    for name in ["TOP_LEVEL_ANSWER", "top_level_answer"] {
        let answer = mrb
            .eval::<beni::Value>(name)
            .expect("the constant and the method must be defined at the top level");
        assert_eq!(i64::from_value(answer), Some(42));
    }
}

#[test]
fn load_string_inside_a_registered_method_hands_the_raise_back_as_err() {
    let mrb = open_mrb();
    mrb.class_object()
        .define_method(&mrb, c"load_inside", beni::method!(load_string_inside, 0))
        .expect("defining the method must succeed");

    let exc = beni::value::qnil()
        .as_value()
        .funcall(&mrb, c"load_inside", &[])
        .expect("the raise must come back to the method that ran the load");

    assert_eq!(exc.classname(&mrb), "RuntimeError");
    assert!(Error::Exception(exc)
        .message(&mrb)
        .contains("from the nested load"));
}

#[test]
fn load_irep_inside_a_registered_method_hands_the_raise_back_as_err() {
    let mrb = open_mrb();
    let bytes = dumped(&mrb, b"raise ArgumentError, 'from the nested program'");
    mrb.class_object()
        .define_method(&mrb, c"load_inside", beni::method!(load_irep_inside, 1))
        .expect("defining the method must succeed");

    let exc = beni::value::qnil()
        .as_value()
        .funcall(&mrb, c"load_inside", &[mrb.str_new(&bytes).as_value()])
        .expect("the raise must come back to the method that ran the load");

    assert_eq!(exc.classname(&mrb), "ArgumentError");
    assert!(Error::Exception(exc)
        .message(&mrb)
        .contains("from the nested program"));
}

#[test]
fn eval_converts_the_program_result_into_the_type_named() {
    let mrb = open_mrb();

    assert_eq!(mrb.eval::<i64>("1 + 2").expect("the sum converts"), 3);
    assert_eq!(
        mrb.eval::<Vec<String>>("%w[a b]")
            .expect("the words convert"),
        ["a", "b"]
    );
    assert!(matches!(
        mrb.eval::<String>("1"),
        Err(beni::Error::Exception(_))
    ));
}
