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
//! Scope is deliberately the scalar leaf types (`i32` / `f64` /
//! `bool`). This is NOT the full magnus hierarchy — no typed-value
//! (`RArray` / `RString`) conversion family and no owned/borrowed
//! split; the `Array` / `Hash` newtypes keep their own `as_value` /
//! `from_value_unchecked` ladder.

use crate::{Mrb, Value};

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

#[cfg(test)]
mod tests {
    use super::*;

    // Boxes through mruby's generic `mrb_int_value` / `mrb_float_value`
    // constructors and unboxes through the macro-expanding C helpers —
    // the full ABI-alignment path. A bindgen/archive layout mismatch
    // (wrong defines fed to the trampoline compile) corrupts these
    // roundtrips before anything else.
    #[test]
    fn scalars_roundtrip_through_a_live_vm() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let int_val = 42i32.into_value(&mrb);
        assert_eq!(i32::from_value(int_val), Some(42));

        let float_val = 1.5f64.into_value(&mrb);
        assert_eq!(f64::from_value(float_val), Some(1.5));

        // Cross-type downcasts fail cleanly instead of misreading the
        // payload.
        assert_eq!(i32::from_value(float_val), None);
        assert_eq!(f64::from_value(int_val), None);
    }
}
