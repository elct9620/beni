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

/// Cross a raw `mrb_value` back into its typed form — magnus's
/// `rb_sys::FromRawValue`. The reading direction needs no trait: a typed
/// handle answers its raw form through its own `as_raw`.
pub trait FromRawValue {
    /// Wrap `value` as the typed form it carries.
    ///
    /// # Safety
    ///
    /// `value` must be one the interpreter it is used against produced.
    /// The typed surface trusts what it is handed rather than re-testing
    /// it, so a value from anywhere else reaches operations that read it
    /// as the thing its tag claims.
    unsafe fn from_raw(value: mrb_value) -> Self;
}

impl FromRawValue for Value {
    #[inline]
    unsafe fn from_raw(value: mrb_value) -> Self {
        // The wrap itself cannot fail; what the caller established is
        // the value's provenance, which nothing here re-tests.
        Value::from_raw_unchecked(value)
    }
}

/// Read the raw interned id out of an `Id` — magnus's `rb_sys::AsRawId`.
pub trait AsRawId: Copy {
    /// The raw id this `Id` carries.
    fn as_raw(self) -> mrb_sym;
}

impl AsRawId for crate::Id {
    #[inline]
    fn as_raw(self) -> mrb_sym {
        self.to_raw()
    }
}

/// Cross a raw interned id back into its typed `Id` — magnus's
/// `rb_sys::FromRawId`.
pub trait FromRawId {
    /// Wrap `id` as the typed `Id`.
    ///
    /// # Safety
    ///
    /// `id` must be one the interpreter the `Id` is used against
    /// interned. An id naming no symbol reifies its name as a value
    /// carrying no String, which the typed surface hands back as one.
    unsafe fn from_raw(id: mrb_sym) -> Self;
}

impl FromRawId for crate::Id {
    #[inline]
    unsafe fn from_raw(id: mrb_sym) -> Self {
        // As `FromRawValue`: the wrap cannot fail, and the id's
        // provenance is the caller's.
        crate::Id::from_raw_unchecked(id)
    }
}
