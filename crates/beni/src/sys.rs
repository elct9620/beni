//! Raw FFI escape hatch: every binding `beni-sys` carries, and the
//! helpers for code working at that layer — magnus's `rb_sys` module.
//!
//! Reach for `beni::sys::mrb_*` when the typed surface does not cover a
//! needed symbol; zeroing such a use is never the goal.

pub use beni_sys::*;

use crate::{Error, Mrb, Value};

/// Run `body` inside mruby's protected frame (`mrb_protect_error`),
/// answering its value, or `Err(Error::Exception)` carrying an exception
/// a raw binding raised inside it, the pending exception cleared.
///
/// Wrap a raw `mrb_*` call that can raise; typed operations already hand
/// their raise back as an `Err`. A raise skips the destructors of the
/// frames it crosses, so the code making the raw call keeps nothing to
/// drop alive in the body when it raises. There is no panic boundary: a
/// panic in `body` aborts the process.
pub fn protect<F>(mrb: &Mrb, body: F) -> Result<Value, Error>
where
    F: FnOnce(&Mrb) -> Value,
{
    mrb.protect(body)
}
