//! What every test here needs before it can ask anything.

use beni::Mrb;

/// A fresh interpreter. Every test opens one, and a failure here is
/// the archive missing rather than the case failing, so the message
/// names that rather than the file name one toolchain gives it.
pub fn open_mrb() -> Mrb {
    Mrb::open().expect("Mrb::open failed — is an mruby archive linked?")
}
