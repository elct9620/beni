//! The interpreter boots, evaluates, and hands a value back — the
//! shortest path that proves the archive, the bindings, and the typed
//! conversions all reach a consumer.

use beni::{FromValue, Mrb};

#[test]
fn an_interpreter_boots_and_evaluates_ruby() {
    let mrb = Mrb::open().expect("Mrb::open needs a staged mruby archive");

    let value = mrb.load_string(b"1 + 2").expect("evaluating must succeed");

    assert_eq!(i32::from_value(value), Some(3));
}
