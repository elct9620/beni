//! What every test here needs before it can ask anything.

use beni::{IntoValue, Mrb};

/// A fresh interpreter. Every test opens one, and a failure here is
/// the archive missing rather than the case failing, so the message
/// names that rather than the file name one toolchain gives it.
pub fn open_mrb() -> Mrb {
    Mrb::open().expect("Mrb::open failed — is an mruby archive linked?")
}

/// Whether two handles name the same object — identity through the
/// values they stand for, the comparison a consumer has for handles
/// that carry no raw pointer of their own.
pub fn same_object(mrb: &Mrb, a: impl IntoValue, b: impl IntoValue) -> bool {
    a.into_value(mrb).obj_equal(mrb, b.into_value(mrb))
}
