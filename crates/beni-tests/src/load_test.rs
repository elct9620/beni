use crate::support::open_mrb;
use beni::{Ccontext, DumpOptions, Error, FromValue, Mrb};

const HEADER_LEN: usize = core::mem::size_of::<beni::sys::rite_binary_header>();

#[test]
fn load_string_evaluates_a_valid_expression_to_its_value() {
    let mrb = open_mrb();

    let got = mrb
        .load_string(b"1 + 2")
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
        .load_string(b"raise 'kaboom'")
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
        .load_string(b"a = 1\nb = 2\nend\n")
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
        .load_string(b"def outer\n  raise 'deep'\nend\nouter\n")
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
        .load_string(b"begin\n  1\nelse\n  2\nend\n")
        .expect("a warning must not change the load's outcome");

    assert_eq!(i32::from_value(got), Some(2));
}

/// The `ScriptError` a blob mruby cannot read as a program comes back
/// as, rendered. Asserts the class as well as the failure, so a load
/// that reported the right words under the wrong class still fails
/// here — the class is what a Ruby-side bare `rescue` reads.
fn structural_failure(mrb: &Mrb, blob: &[u8]) -> String {
    let err = mrb
        .load_bytecode(blob)
        .expect_err("a blob that is not a program must not load");
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
fn load_bytecode_classifies_a_blob_shorter_than_the_header() {
    let mrb = open_mrb();

    assert!(structural_failure(&mrb, b"RITE").contains("shorter than RITE binary header"));
}

#[test]
fn load_bytecode_classifies_a_non_rite_ident() {
    let mrb = open_mrb();

    assert!(structural_failure(&mrb, &[b'X'; HEADER_LEN]).contains("not RITE format"));
}

#[test]
fn load_bytecode_classifies_a_rite_version_mismatch() {
    let mrb = open_mrb();
    let mut blob = [0u8; HEADER_LEN];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(b"0000");

    assert!(structural_failure(&mrb, &blob).contains("RITE version mismatch"));
}

#[test]
fn load_bytecode_classifies_a_corrupt_body() {
    let mrb = open_mrb();
    let mut blob = [0u8; HEADER_LEN + 8];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(&beni::sys::RITE_BINARY_FORMAT_VER[..4]);

    assert!(structural_failure(&mrb, &blob).contains("failed structural validation"));
}

#[test]
fn load_bytecode_leaves_the_vm_usable_after_a_structural_failure() {
    let mrb = open_mrb();

    let _ = structural_failure(&mrb, b"not RITE bytecode");

    let alive = mrb
        .load_string(b"1 + 1")
        .expect("the VM survives a load that never became a program");
    assert_eq!(i32::from_value(alive), Some(2));
}

#[test]
fn load_bytecode_hands_back_an_exception_the_program_raised() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"raiser.rb").expect("allocating the context must succeed");
    let bytes = cxt
        .compile(b"raise ArgumentError, 'from the loaded program'")
        .expect("the source must compile")
        .dump(&mrb, DumpOptions::default())
        .expect("a Proc compiled from source must dump");

    let err = mrb
        .load_bytecode(&bytes)
        .expect_err("a program that raises must not come back Ok");

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
