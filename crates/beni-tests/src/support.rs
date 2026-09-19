//! What every test here needs before it can ask anything.

use beni::{FromValue, IntoValue, Mrb, RString, ReprValue};

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

/// A string's bytes as an owned `Vec<u8>`, the way a consumer without
/// the `bytes` feature reads them — through the `FromValue` conversion.
pub trait OwnedBytes {
    fn owned_bytes(self) -> Vec<u8>;
}

impl OwnedBytes for RString {
    fn owned_bytes(self) -> Vec<u8> {
        Vec::<u8>::from_value(self.as_value()).expect("a string handle is String-tagged")
    }
}
