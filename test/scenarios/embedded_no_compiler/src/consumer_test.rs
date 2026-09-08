//! The embedded consumer's whole journey: boot an interpreter against
//! an archive with no compiler in it, install the gem, and reach the
//! method from Rust — the dispatch a consumer uses where there is no
//! Ruby source to evaluate.

use crate::Doubler;
use beni::{FromValue, IntoValue, Mrb};

#[test]
fn a_gem_answers_where_no_compiler_is_linked() {
    let mrb = Mrb::open().expect("Mrb::open needs the archive beni:build staged here");
    mrb.init_gem::<Doubler>()
        .expect("installing the gem must succeed");

    let class = mrb
        .class_get(c"Doubler")
        .expect("the gem defined the class");
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"call", &[21.into_value(&mrb)])
        .expect("dispatching to the gem's method must not raise");

    assert_eq!(i32::from_value(got), Some(42));
}
