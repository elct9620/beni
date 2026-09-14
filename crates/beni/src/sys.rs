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
/// their raise back as an `Err`. A raise leaves the body without returning
/// through it and may skip its destructors, so the code making the raw
/// call keeps nothing that could need dropping alive in the body when it
/// raises. There is no panic boundary: a panic in `body` aborts the
/// process.
pub fn protect<F>(mrb: &Mrb, body: F) -> Result<Value, Error>
where
    F: FnOnce(&Mrb) -> Value,
{
    mrb.protect(body)
}

/// Run `func`, answering its value, or `Err(Error::Panic)` carrying the
/// message of a panic inside it — magnus's `rb_sys::catch_unwind`.
///
/// Wrap the body of a Rust closure handed to a raw binding as a C
/// callback, so its panic stops here instead of unwinding into mruby's
/// frames.
pub fn catch_unwind<F, T>(func: F) -> Result<T, Error>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(func)
        .map_err(|payload| Error::Panic(crate::error::panic_message(payload)))
}
