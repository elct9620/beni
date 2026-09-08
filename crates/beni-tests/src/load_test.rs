use beni::{Error, FromValue, Mrb};

const HEADER_LEN: usize = core::mem::size_of::<beni::sys::rite_binary_header>();

#[test]
fn load_string_evaluates_a_valid_expression_to_its_value() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let err = mrb
        .load_string(b"raise 'kaboom'")
        .expect_err("a raising script must come back Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("kaboom")),
        Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "the pending exception must be cleared as the Err crosses out"
    );
}

#[test]
fn load_string_surfaces_a_syntax_error_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let err = mrb
        .load_string(b"def (")
        .expect_err("unparseable source must come back Err");

    assert!(matches!(err, Error::Exception(_)));
    assert!(
        mrb.pending_exc().is_nil(),
        "the pending exception must be cleared as the Err crosses out"
    );
}

/// The synthesised `RuntimeError` parked under `mrb->exc`, rendered.
fn exc_message(mrb: &Mrb) -> String {
    let exc = mrb.pending_exc();
    assert!(
        !exc.is_nil(),
        "a structural failure must synthesise mrb->exc"
    );
    exc.to_string(mrb)
}

#[test]
fn load_bytecode_classifies_a_blob_shorter_than_the_header() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    assert_eq!(mrb.load_bytecode(b"RITE"), 1);
    assert!(exc_message(&mrb).contains("shorter than RITE binary header"));
}

#[test]
fn load_bytecode_classifies_a_non_rite_ident() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    assert_eq!(mrb.load_bytecode(&[b'X'; HEADER_LEN]), 1);
    assert!(exc_message(&mrb).contains("not RITE format"));
}

#[test]
fn load_bytecode_classifies_a_rite_version_mismatch() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let mut blob = [0u8; HEADER_LEN];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(b"0000");

    assert_eq!(mrb.load_bytecode(&blob), 1);
    assert!(exc_message(&mrb).contains("RITE version mismatch"));
}

#[test]
fn load_bytecode_classifies_a_corrupt_body() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let mut blob = [0u8; HEADER_LEN + 8];
    blob[..4].copy_from_slice(&beni::sys::RITE_BINARY_IDENT[..4]);
    blob[4..8].copy_from_slice(&beni::sys::RITE_BINARY_FORMAT_VER[..4]);

    assert_eq!(mrb.load_bytecode(&blob), 1);
    assert!(exc_message(&mrb).contains("failed structural validation"));
}

#[test]
fn load_irep_buf_leaves_the_pending_exception_for_a_malformed_blob() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // The documented contract: a malformed blob sets `mrb->exc`
    // for the caller to inspect through `pending_exc`, and the
    // VM stays usable once the caller clears it.
    let _ = mrb.load_irep_buf(b"not RITE bytecode");
    assert!(
        !mrb.pending_exc().is_nil(),
        "a malformed blob must leave a pending exception"
    );
    mrb.clear_exc();
    assert!(mrb.pending_exc().is_nil());

    let alive = mrb
        .load_string(b"1 + 1")
        .expect("the VM survives the cleared load failure");
    assert_eq!(i32::from_value(alive), Some(2));
}
