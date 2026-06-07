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
//! Scope covers the scalar leaf types (`i32` / `f64` / `bool`) and
//! checked downcasts to the container newtypes (`Array` / `Hash`),
//! discriminated by the value's type tag — subclass instances
//! convert. No owned/borrowed split — every conversion is by value.

use crate::{Array, Hash, Mrb, Value};

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

#[cfg(all(test, mruby_linked))]
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

    #[test]
    fn container_downcasts_discriminate_by_tag() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let ary = mrb.ary_new().as_value();
        let hash = mrb.hash_new().as_value();

        assert!(Array::from_value(ary).is_some());
        assert!(Hash::from_value(hash).is_some());

        // The wrong container tag — and a non-container tag — both
        // reject instead of wrapping a value the `mrb_ary_*` /
        // `mrb_hash_*` calls would misread.
        assert!(Array::from_value(hash).is_none());
        assert!(Hash::from_value(ary).is_none());
        assert!(Array::from_value(42i32.into_value(&mrb)).is_none());
        assert!(Hash::from_value(42i32.into_value(&mrb)).is_none());
    }

    #[test]
    fn container_downcast_includes_subclass_instances() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = crate::Ccontext::new(&mrb, c"convert_test.rb")
            .expect("allocating the compile context must succeed");

        let sub = cxt.load_nstring(b"class MyAry < Array; end; MyAry.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the Array subclass must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // The tag, not the classname, decides: the instance reports
        // its subclass name yet converts and operates as an Array.
        assert_eq!(sub.classname(&mrb), "MyAry");
        let ary = Array::from_value(sub).expect("subclass instance carries MRB_TT_ARRAY");
        ary.push(&mrb, mrb.str_new(b"x"));
        assert_eq!(ary.entry(0).to_string(&mrb), "x");
    }
}
