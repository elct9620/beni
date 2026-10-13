//! A method's `Err` raised to its Ruby caller in a build whose panic
//! strategy is abort, the profile tests cannot take. The raise crosses
//! the method bridge, so a process that reaches its end is one whose
//! long-jump met nothing that aborts it.

use beni::{Ccontext, FromValue, Mrb};
use rust_consumer::Doubler;

fn main() {
    let mrb = Mrb::open().expect("Mrb::open needs the archive beni:build staged here");
    mrb.init_gem::<Doubler>()
        .expect("installing the gem must succeed");

    let cxt = Ccontext::new(&mrb, c"consumer.rb").expect("allocating the context must succeed");
    let got = cxt
        .load_nstring(b"begin; Doubler.new.call('21'); rescue TypeError; 1; end")
        .expect("the raise must reach the Ruby rescue");

    assert_eq!(i32::from_value(got), Some(1));
}
