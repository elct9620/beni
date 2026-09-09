//! The half of the universal binary this host can run: boot an
//! interpreter against the archive built for both architectures,
//! install the gem, and reach it from Ruby source.

use crate::Doubler;
use beni::{Ccontext, FromValue, Mrb};

#[test]
fn a_gem_answers_from_the_slice_this_host_runs() {
    let mrb = Mrb::open().expect("Mrb::open needs the archive beni:build staged here");
    mrb.init_gem::<Doubler>()
        .expect("installing the gem must succeed");

    let cxt = Ccontext::new(&mrb, c"consumer.rb").expect("allocating the context must succeed");
    let got = cxt
        .load_nstring(b"Doubler.new.call(21)")
        .expect("evaluating the gem surface must compile and run");

    assert_eq!(i32::from_value(got), Some(42));
}
