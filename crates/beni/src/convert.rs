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
    fn bool_round_trips_and_converts_totally() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // The two canonical booleans round-trip through IntoValue.
        assert_eq!(bool::from_value(true.into_value(&mrb)), Some(true));
        assert_eq!(bool::from_value(false.into_value(&mrb)), Some(false));
        // The conversion is total — it never returns None — and reads a
        // non-boolean through Ruby truthiness (`nil` is falsy). The full
        // truthiness boundary lives with `Value::to_bool` in value.rs.
        assert_eq!(bool::from_value(Value::nil()), Some(false));
    }

    #[test]
    fn string_converts_utf8_and_rejects_otherwise() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A UTF-8 string value converts to an owned Rust String,
        // multi-byte characters included.
        let s = mrb.str_new("héllo".as_bytes()).as_value();
        assert_eq!(String::from_value(s), Some("héllo".to_string()));

        // A non-string tag rejects.
        assert_eq!(String::from_value(42i32.into_value(&mrb)), None);

        // A String-tagged value whose bytes are not valid UTF-8 rejects
        // — it cannot become a Rust String, whose invariant is UTF-8.
        let invalid = mrb.str_new(&[0xff, 0xfe]).as_value();
        assert_eq!(String::from_value(invalid), None);
    }

    #[test]
    fn rstring_downcasts_by_tag() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A String-tagged value downcasts to the typed handle; a
        // non-string tag rejects instead of wrapping a value the
        // `mrb_str_*` calls would misread.
        let s = mrb.str_new(b"hi").as_value();
        assert_eq!(
            RString::from_value(s).map(|r| r.to_bytes()),
            Some(b"hi".to_vec())
        );
        assert!(RString::from_value(42i32.into_value(&mrb)).is_none());
    }

    #[test]
    fn vec_u8_converts_arbitrary_bytes_and_rejects_non_string() {
        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A String-tagged value yields its bytes verbatim — non-UTF-8
        // bytes that the owned `String` conversion rejects survive here.
        let binary = mrb.str_new(&[0xff, 0x00, 0xfe]).as_value();
        assert_eq!(Vec::<u8>::from_value(binary), Some(vec![0xff, 0x00, 0xfe]));
        // A non-string tag rejects, like every other downcast.
        assert_eq!(Vec::<u8>::from_value(42i32.into_value(&mrb)), None);
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
        ary.push(&mrb, mrb.str_new(b"x").as_value())
            .expect("push to a fresh array succeeds");
        assert_eq!(ary.entry(0).to_string(&mrb), "x");
    }

    #[test]
    fn class_downcast_admits_only_the_class_tag() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = crate::Ccontext::new(&mrb, c"convert_test.rb")
            .expect("allocating the compile context must succeed");

        let class_val = cxt.load_nstring(b"String");
        let module_val = cxt.load_nstring(b"Kernel");
        assert!(
            mrb.pending_exc().is_nil(),
            "looking up the constants must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        let class = RClass::from_value(class_val).expect("a Class value carries MRB_TT_CLASS");
        assert_eq!(class.name(&mrb), "String");

        // Modules and non-class values reject — MRB_TT_MODULE is not
        // the class tag.
        assert!(RClass::from_value(module_val).is_none());
        assert!(RClass::from_value(42i32.into_value(&mrb)).is_none());
    }
}
