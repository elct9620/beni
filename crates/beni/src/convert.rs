//! Rust ↔ mruby `Value` conversion traits — the typed layer over the
//! raw boxing / unboxing primitives in `value.rs`.
//!
//! This is beni's small slice of the magnus conversion contract:
//! `IntoValue` mirrors magnus's `IntoValue` (Rust → value, infallible
//! boxing), `FromValue` mirrors magnus's `TryConvert` (value → Rust,
//! fallible downcast). Both sit ON TOP of the unsafe tag primitives in
//! `value.rs` (`Value::from_int` / `is_integer` + `unbox_integer` / …):
//! those primitives are the C-bind floor, these traits are the safe
//! typed seam consumers call.
//!
//! Scope covers the scalar leaf types (`i32` / `f64` / `bool`), an
//! owned `String` or byte vector, and checked downcasts to the typed
//! handles (`RString` / `Array` / `Hash` / `RClass` / `Proc` /
//! `Symbol` / `Range`), discriminated by the value's type tag — string
//! and container subclass instances convert. Every conversion is by
//! value, copying rather than borrowing VM storage.

use crate::{Array, Hash, Mrb, Proc, RClass, RString, Range, Symbol, Value};

/// Box a Rust value into an mruby `Value`. Infallible — every
/// implementor has a total mapping into the value domain. Mirrors
/// magnus's `IntoValue`; the call shape is `n.into_value(mrb)`.
pub trait IntoValue {
    fn into_value(self, mrb: &Mrb) -> Value;
}

/// Downcast an mruby `Value` to a Rust type, returning `None` when
/// the value is not tagged as the target type. Safe: the tag check is
/// folded in, so callers no longer pair a predicate with an `unsafe`
/// unbox. Mirrors magnus's `TryConvert`; named `FromValue` here for the
/// `T::from_value(v)` call shape.
pub trait FromValue: Sized {
    fn from_value(value: Value) -> Option<Self>;
}

impl IntoValue for Value {
    // Identity — a value is already in the value domain. Lets
    // bridge-shaped functions that produce a raw `Value` satisfy the
    // same return seam as the scalar conversions.
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self
    }
}

impl IntoValue for i32 {
    // `sys::mrb_int` follows the archive's config: the conversion is
    // a lossless widening under 64-bit width and an identity under
    // `MRB_INT32` — clippy only sees the latter when checking against
    // a 32-bit-pinned archive, hence the targeted allow.
    #[allow(clippy::useless_conversion)]
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_int(mrb, self.into())
    }
}

impl IntoValue for f64 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_float(mrb, self)
    }
}

impl IntoValue for bool {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        if self {
            Value::true_()
        } else {
            Value::false_()
        }
    }
}

impl IntoValue for Symbol {
    // A `Symbol` already wraps its Symbol-tagged `Value`; boxing is the
    // identity unwrap, like `IntoValue for Value`.
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl FromValue for i32 {
    // Mirror of the `IntoValue for i32` allow: `try_from` is a real
    // range check under 64-bit `sys::mrb_int` and an infallible
    // identity under `MRB_INT32`.
    #[allow(clippy::useless_conversion)]
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        if !value.is_integer() {
            return None;
        }
        // SAFETY: the unbox precondition (MRB_TT_INTEGER tagging) is
        // established by the `is_integer` guard immediately above.
        let raw = unsafe { value.unbox_integer() };
        // `sys::mrb_int` follows the archive's configured width; when
        // it is 64-bit (mruby's 64-bit platform default) an
        // out-of-i32-range integer is not representable — downcast
        // failure, same contract as a type-tag mismatch.
        Self::try_from(raw).ok()
    }
}

impl FromValue for f64 {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (MRB_TT_FLOAT tagging) is
        // established by the `is_float` guard immediately before it.
        value.is_float().then(|| unsafe { value.unbox_float() })
    }
}

impl FromValue for bool {
    // Ruby truthiness, not a tag check: `nil` and `false` read as
    // `false`, every other value as `true`. Total — always `Some` —
    // mirroring magnus's `TryConvert for bool` (`Ok(val.to_bool())`).
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Some(value.to_bool())
    }
}

impl FromValue for Array {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_ARRAY tagging) is
        // established by the `is_array` guard immediately before it.
        value
            .is_array()
            .then(|| unsafe { Array::from_value_unchecked(value) })
    }
}

impl FromValue for Hash {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_HASH tagging) is
        // established by the `is_hash` guard immediately before it.
        value
            .is_hash()
            .then(|| unsafe { Hash::from_value_unchecked(value) })
    }
}

impl FromValue for RClass {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (class tagging) is
        // established by the `is_class` guard immediately before it.
        value
            .is_class()
            .then(|| RClass::from_raw(unsafe { value.as_class_ptr() }))
    }
}

impl FromValue for Proc {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_PROC tagging) is
        // established by the `is_proc` guard immediately before it.
        value
            .is_proc()
            .then(|| unsafe { Proc::from_value_unchecked(value) })
    }
}

impl FromValue for Symbol {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_SYMBOL tagging) is
        // established by the `is_symbol` guard immediately before it.
        value
            .is_symbol()
            .then(|| unsafe { Symbol::from_value_unchecked(value) })
    }
}

impl FromValue for RString {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_STRING tagging) is
        // established by the `is_string` guard immediately before it.
        value
            .is_string()
            .then(|| unsafe { RString::from_value_unchecked(value) })
    }
}

impl FromValue for Range {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_RANGE tagging) is
        // established by the `is_range` guard immediately before it.
        value
            .is_range()
            .then(|| unsafe { Range::from_value_unchecked(value) })
    }
}

impl FromValue for String {
    // A String-tagged value whose bytes are valid UTF-8 converts; a
    // non-string tag and a non-UTF-8 string both reject — a Rust
    // `String` is UTF-8 by invariant, so non-UTF-8 bytes genuinely
    // cannot become one. Mirrors magnus's `TryConvert for String`.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Self::from_utf8(RString::from_value(value)?.to_bytes()).ok()
    }
}

impl FromValue for Vec<u8> {
    // A String-tagged value yields its bytes verbatim — arbitrary, not
    // required to be UTF-8 — the binary counterpart to the owned
    // `String` conversion for callers that handle raw byte strings. A
    // non-string tag rejects.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Some(RString::from_value(value)?.to_bytes())
    }
}
