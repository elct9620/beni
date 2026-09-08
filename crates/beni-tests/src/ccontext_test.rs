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

#[test]
fn warnings_carry_the_compiler_diagnostics_a_load_produced() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    // `else` on a `begin` with no `rescue` never runs; mruby warns.
    let got = cxt
        .load_nstring(b"begin\n  1\nelse\n  2\nend\n")
        .expect("a warning must not change the load's outcome");

    assert_eq!(i32::from_value(got), Some(2), "the useless else still runs");
    let warnings = cxt.warnings();
    assert_eq!(warnings.len(), 1, "got {warnings:?}");
    assert!(
        warnings[0].message().contains("else without rescue"),
        "unexpected warning: {}",
        warnings[0].message()
    );
    // mruby resolves the `begin` node at its `end`, so that is where it
    // places the warning.
    assert_eq!(warnings[0].line(), 5);
}

#[test]
fn warnings_answer_empty_before_a_load_and_after_a_clean_one() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    assert!(cxt.warnings().is_empty(), "a context that has run no load");

    cxt.load_nstring(b"begin\n  1\nelse\n  2\nend\n")
        .expect("the warning source must still run");
    assert_eq!(cxt.warnings().len(), 1);

    cxt.load_nstring(b"1 + 1").expect("clean source must run");
    assert!(
        cxt.warnings().is_empty(),
        "warnings answer the most recent load, not every load"
    );
}
