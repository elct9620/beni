use beni::{Ccontext, Error, FromValue, IntoValue, Mrb, Proc, Value};

fn proc_from(mrb: &Mrb, src: &[u8]) -> Proc {
    let cxt =
        Ccontext::new(mrb, c"proc_test.rb").expect("allocating the compile context must succeed");
    let value = cxt
        .load_nstring(src)
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "compiling the proc literal must not raise: {}",
        mrb.pending_exc().to_string(mrb)
    );
    Proc::from_value(value).expect("a proc literal carries MRB_TT_PROC")
}

#[test]
fn call_yields_to_the_block_and_returns_its_value() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let block = proc_from(&mrb, b"proc { |x| x + 1 }");

    let got = block
        .call(&mrb, &[41i32.into_value(&mrb)])
        .expect("yielding a non-raising block must come back Ok");

    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn call_surfaces_a_raised_exception_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let block = proc_from(&mrb, b"proc { raise 'boom from block' }");

    let err = block
        .call(&mrb, &[])
        .expect_err("a raise inside the block must surface as Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("boom from block")),
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }
    // The VM stays usable after the protected raise.
    let again = proc_from(&mrb, b"proc { 7 }");
    let got = again
        .call(&mrb, &[])
        .expect("the VM must survive the protected raise");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn from_value_rejects_a_non_proc_value() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A scalar carries no MRB_TT_PROC tag — the downcast rejects
    // instead of wrapping a value `mrb_yield_argv` would misread.
    assert!(Proc::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn as_value_round_trips_through_the_newtype() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let block = proc_from(&mrb, b"proc { 0 }");

    // The reified value is still Proc-tagged and downcasts back.
    let value: Value = block.as_value();
    assert!(Proc::from_value(value).is_some());
}
