//! Rust ↔ mruby `Value` conversion traits — the typed layer over the
//! raw boxing / unboxing primitives in `value.rs`.
//!
//! This is beni's small slice of the magnus conversion contract:
//! `IntoValue` mirrors magnus's `IntoValue` (Rust → value, infallible
//! boxing), `FromValue` mirrors magnus's `TryConvert` (value → Rust,
//! fallible downcast). Both sit ON TOP of the unsafe tag primitives in
//! `value.rs` (`mrb_int_value` / `is_integer` + `unbox_integer` / …):
//! those primitives are the C-bind floor, these traits are the safe
//! typed seam consumers call.
//!
//! Scope covers `Value` itself, the scalar leaf types (the Rust
//! integers, `f64`, `bool`), an owned `String` or byte vector, the
//! typed handles (`RString` / `Array` / `Hash` / `RClass` / `RModule` /
//! `ExceptionClass` / `Proc` / `Symbol` / `Range`), and an `Option` of
//! any of them that reads `nil` as `None`: every handle converts into
//! the value naming its object, and back through a checked downcast
//! discriminated by the value's type tag — string and container
//! subclass instances convert. Every conversion is by value, copying
//! rather than borrowing VM storage.

use crate::{
    sys, Array, ExceptionClass, Hash, Mrb, Proc, RClass, RModule, RString, Range, Symbol, Value,
};

/// Box a Rust value into an mruby `Value`. Infallible — every
/// implementor has a total mapping into the value domain. Mirrors
/// magnus's `IntoValue`; the call shape is `n.into_value(mrb)`.
///
/// A Rust integer converts only where every value it holds fits the
/// archive's configured integer width: `i8` / `i16` / `i32` / `u8` /
/// `u16` under every width, `u32` / `i64` under a 64-bit width, and
/// `isize` wherever the target's pointer width fits the configured one.
/// Rendered documentation shows the 64-bit set.
///
/// ```
/// fn converts<T: beni::IntoValue>() {}
/// converts::<i8>();
/// converts::<i16>();
/// converts::<i32>();
/// converts::<u8>();
/// converts::<u16>();
/// #[cfg(mrb_int64)]
/// {
///     converts::<u32>();
///     converts::<i64>();
///     converts::<isize>();
/// }
/// ```
///
/// A 32-bit width does not hold every `i64`:
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// #[cfg(mrb_int64)]
/// compile_error!("a 64-bit width holds every i64");
/// converts::<i64>();
/// ```
///
/// No width holds every `u64` or `usize`:
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// converts::<u64>();
/// ```
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// converts::<usize>();
/// ```
pub trait IntoValue {
    fn into_value(self, mrb: &Mrb) -> Value;
}

/// Downcast an mruby `Value` to a Rust type, returning `None` when
/// the value is not tagged as the target type or, for a Rust integer,
/// carries an Integer outside the target's own range. Safe: the tag check is
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

macro_rules! into_value_widening {
    ($($int:ty),* $(,)?) => {$(
        impl IntoValue for $int {
            #[inline]
            fn into_value(self, mrb: &Mrb) -> Value {
                Value::from_int(mrb, sys::mrb_int::from(self))
            }
        }
    )*};
}

into_value_widening!(i8, i16, i32, u8, u16);
#[cfg(mrb_int64)]
into_value_widening!(u32, i64);

#[cfg(any(mrb_int64, not(target_pointer_width = "64")))]
impl IntoValue for isize {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        // The cfg admits only a pointer width the configured integer
        // width holds, so the cast never truncates.
        Value::from_int(mrb, self as sys::mrb_int)
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

// A handle on a Ruby object converts into the value naming that same
// object: the `Value`-newtype handles unwrap it, the class handles box
// their pointer.
impl IntoValue for Symbol {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for RString {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for Array {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for Hash {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for Proc {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for Range {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        self.as_value()
    }
}

impl IntoValue for RClass {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.to_value(mrb)
    }
}

impl IntoValue for RModule {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.to_value(mrb)
    }
}

impl IntoValue for ExceptionClass {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.to_value(mrb)
    }
}

impl FromValue for Value {
    // Identity, the counterpart of `IntoValue for Value`: a parameter
    // that accepts any value converts without rejecting one.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Some(value)
    }
}

impl<T: FromValue> FromValue for Option<T> {
    // `nil` reads as absent, mruby's `!` nilable read; any other value
    // converts by `T`'s rule, so what `T` rejects stays rejected.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        if value.is_nil() {
            Some(None)
        } else {
            T::from_value(value).map(Some)
        }
    }
}

/// The integer an Integer-tagged `value` carries, or `None` for any
/// other tag.
#[inline]
fn integer(value: Value) -> Option<i64> {
    // SAFETY: the unbox precondition (MRB_TT_INTEGER tagging) is
    // established by the `is_integer` guard it runs behind.
    value.is_integer().then(|| unsafe { value.unbox_integer() })
}

macro_rules! from_value_in_range {
    ($($int:ty),* $(,)?) => {$(
        impl FromValue for $int {
            #[inline]
            fn from_value(value: Value) -> Option<Self> {
                integer(value).and_then(|n| <$int>::try_from(n).ok())
            }
        }
    )*};
}

from_value_in_range!(i8, i16, i32, u8, u16, u32, u64, isize, usize);

impl FromValue for i64 {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        integer(value)
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
    // A singleton class is a class handle too — `Value::singleton_class`
    // hands one out — so both class tags convert.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (class or singleton-class
        // tagging) is established by the guard immediately before it.
        (value.is_class() || value.is_sclass())
            .then(|| RClass::from_raw(unsafe { value.as_class_ptr() }))
    }
}

impl FromValue for RModule {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (MRB_TT_MODULE tagging) is
        // established by the `is_module` guard immediately before it.
        value
            .is_module()
            .then(|| RModule::from_raw(unsafe { value.as_class_ptr() }))
    }
}

impl FromValue for ExceptionClass {
    // Narrower than its tag: a class converts only when it is an
    // exception class, which a singleton class never is.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        if !value.is_class() {
            return None;
        }
        // SAFETY: the unbox precondition (class tagging) is established
        // by the `is_class` guard immediately above.
        let class = unsafe { value.as_class_ptr() };
        crate::class::is_exception_class(class).then(|| ExceptionClass::from_raw_unchecked(class))
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
