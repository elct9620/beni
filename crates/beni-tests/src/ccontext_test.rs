use beni::{Ccontext, Error, FromValue, Mrb};

#[test]
fn load_nstring_evaluates_source_under_the_stamped_filename() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let got = cxt
        .load_nstring(b"1 + 1")
        .expect("plain arithmetic must compile and run");

    assert_eq!(i32::from_value(got), Some(2));
}

#[test]
fn load_nstring_surfaces_a_raise_as_an_exception_error() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let err = cxt
        .load_nstring(b"raise 'kaboom'")
        .expect_err("a raise must surface as Err");

    match err {
        Error::Exception(exc) => {
            assert_eq!(exc.classname(&mrb), "RuntimeError");
            assert!(Error::Exception(exc).message(&mrb).contains("kaboom"));
        }
        other => panic!("a raise must surface as Error::Exception, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "the exception crosses out as Err with the handle left clean"
    );
}

#[test]
fn load_nstring_surfaces_a_parse_failure_with_its_location() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    // The `end` on line 3 closes nothing.
    let err = cxt
        .load_nstring(b"a = 1\nb = 2\nend\n")
        .expect_err("source that does not parse must surface as Err");

    match err {
        Error::Syntax(parse) => {
            assert_eq!(parse.line(), 3, "the diagnostic points at the stray `end`");
            assert!(
                !parse.message().is_empty(),
                "the compiler's diagnostic text is carried, not discarded"
            );
        }
        other => panic!("a parse failure must surface as Error::Syntax, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "a program that never compiled leaves no exception on the handle"
    );
}

#[test]
fn a_parse_failure_and_a_raise_are_distinguishable_without_reading_a_message() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let unparsable = cxt
        .load_nstring(b"def")
        .expect_err("source that does not parse must surface as Err");
    let raised = cxt
        .load_nstring(b"raise 'kaboom'")
        .expect_err("a raise must surface as Err");

    assert!(matches!(unparsable, Error::Syntax(_)));
    assert!(matches!(raised, Error::Exception(_)));
}

#[test]
fn one_context_carries_top_level_locals_across_loads() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.load_nstring(b"carried = 7")
        .expect("the assignment must compile and run");
    let got = cxt
        .load_nstring(b"carried * 6")
        .expect("the second load sees the first load's local");

    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn a_context_survives_a_parse_failure_and_keeps_loading() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.load_nstring(b"end")
        .expect_err("source that does not parse must surface as Err");
    let got = cxt
        .load_nstring(b"1 + 1")
        .expect("the context is still usable after a parse failure");

    assert_eq!(i32::from_value(got), Some(2));
}
