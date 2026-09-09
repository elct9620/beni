use crate::support::open_mrb;
use beni::{Ccontext, Error, FromValue};

#[test]
fn load_nstring_evaluates_source_under_the_stamped_filename() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let got = cxt
        .load_nstring(b"1 + 1")
        .expect("plain arithmetic must compile and run");

    assert_eq!(i32::from_value(got), Some(2));
}

#[test]
fn load_nstring_surfaces_a_raise_as_an_exception_error() {
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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
    let mrb = open_mrb();
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

#[test]
fn compile_yields_a_program_that_has_not_run_yet() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let program = cxt
        .compile(b"$ran = true; 7")
        .expect("plain source must compile");

    let ran = mrb.intern_cstr(c"$ran");
    assert!(
        mrb.gv_get(ran).is_nil(),
        "compiling must not run what it compiled"
    );
    let got = program
        .call(&mrb, &[])
        .unwrap_or_else(|_| panic!("the program must run when called"));
    assert_eq!(i32::from_value(got), Some(7));
    assert!(mrb.gv_get(ran).is_true(), "calling runs the program");
}

#[test]
fn compile_surfaces_a_parse_failure_the_way_a_load_does() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let Err(err) = cxt.compile(b"def broken\n") else {
        panic!("source that does not parse must surface as Err");
    };

    match err {
        Error::Syntax(message) => assert!(
            message.line() > 0,
            "the diagnostic carries the line it points at"
        ),
        other => panic!("a parse failure must surface as Error::Syntax, got {other}"),
    }
}

#[test]
fn compile_leaves_the_context_running_its_next_load() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.compile(b"1").expect("plain source must compile");
    let got = cxt
        .load_nstring(b"$after = 3")
        .expect("the next load must run rather than compile");

    assert_eq!(i32::from_value(got), Some(3));
    assert_eq!(
        i32::from_value(mrb.gv_get(mrb.intern_cstr(c"$after"))),
        Some(3),
        "stopping before the run is settled per call, never kept on the context"
    );
}

#[test]
fn compile_records_the_warnings_it_produced() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.compile(b"a = 1\na = 2\n").expect("source must compile");

    assert!(
        cxt.warnings().iter().all(|w| !w.message().is_empty()),
        "a warning the compiler recorded carries its text"
    );
}

#[test]
fn a_compiled_program_starts_without_the_contexts_top_level_locals() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.load_nstring(b"x = 5").expect("the local must be set");
    let seen_by_a_load = cxt
        .load_nstring(b"x")
        .expect("a load through the same context sees the local");
    let program = cxt.compile(b"x").expect("the same source must compile");
    let seen_by_the_program = program
        .call(&mrb, &[])
        .unwrap_or_else(|_| panic!("the program must run when called"));

    assert_eq!(i32::from_value(seen_by_a_load), Some(5));
    assert!(
        seen_by_the_program.is_nil(),
        "the context's locals reach a program it runs, not one the caller runs"
    );
}
