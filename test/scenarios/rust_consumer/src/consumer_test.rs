//! The consumer's whole journey: boot an interpreter against the
//! staged archive, install the gem, and reach it from Ruby source.

use crate::Doubler;
use beni::{Ccontext, FromValue, Mrb};

#[test]
fn a_gem_installed_by_a_consumer_answers_from_ruby() {
    let mrb = Mrb::open().expect("Mrb::open needs the archive beni:build staged here");
    mrb.init_gem::<Doubler>().expect("installing the gem must succeed");

    let cxt = Ccontext::new(&mrb, c"consumer.rb").expect("allocating the context must succeed");
    let got = cxt.load_nstring(b"Doubler.new.call(21)");

    assert!(
        mrb.pending_exc().is_nil(),
        "evaluating the gem surface must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(42));
}
