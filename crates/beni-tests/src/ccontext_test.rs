use beni::Ccontext;
use beni::{FromValue, Mrb};

#[test]
fn load_nstring_evaluates_source_under_the_stamped_filename() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    let got = cxt.load_nstring(b"1 + 1");

    assert!(
        mrb.pending_exc().is_nil(),
        "evaluating plain arithmetic must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(2));
}

#[test]
fn load_nstring_parks_a_raise_in_pending_exc() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"ccontext_test.rb")
        .expect("allocating the compile context must succeed");

    cxt.load_nstring(b"raise 'kaboom'");

    assert!(mrb.pending_exc().to_string(&mrb).contains("kaboom"));
}
