//! Rust ↔ mruby `Value` conversion traits — the typed layer over the
//! raw boxing / unboxing primitives in `value.rs`.
//!
//! This is beni's small slice of the magnus conversion contract:
//! `IntoValue` mirrors magnus's `IntoValue` (Rust → value, infallible
//! boxing), `FromValue` mirrors magnus's `from_value` (value → Rust,
//! exact-tag downcast); the argument conversion, magnus's `TryConvert`,
//! lives in `try_convert`. Both are the safe seam over the tag checks and
//! unchecked unboxing a conversion runs behind.
//!
//! The traits and the conversions of `Value` itself, the Rust scalars,
//! `String`, byte vectors, and `Option`, which reads `nil` as `None`. A
//! typed handle's conversions sit beside the handle, and every conversion
//! copies rather than borrowing VM storage.

use crate::{sys, Mrb, RString, ReprValue, Value};

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
///
/// A Rust float converts by the same rule against the configured float
/// width: `f32` under every width, `f64` under a 64-bit width. Rendered
/// documentation shows the 64-bit set.
///
/// ```
/// fn converts<T: beni::IntoValue>() {}
/// converts::<f32>();
/// #[cfg(not(mrb_float32))]
/// converts::<f64>();
/// ```
///
/// A 32-bit float width does not hold every `f64`:
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// #[cfg(not(mrb_float32))]
/// compile_error!("a 64-bit float width holds every f64");
/// converts::<f64>();
/// ```
pub trait IntoValue {
    fn into_value(self, mrb: &Mrb) -> Value;
}

/// Downcast an mruby `Value` to a Rust type, returning `None` when
/// the value is not tagged as the target type or, for a Rust integer,
/// carries an Integer outside the target's own range. Safe: the tag check is
/// folded in, so callers no longer pair a predicate with an `unsafe`
/// unbox. Mirrors magnus's `from_value`, gathered into one trait for the
/// `T::from_value(v)` call shape; `TryConvert` is the fallible conversion
/// that surfaces mruby's exception instead.
///
/// A float target converts only where it holds every value the
/// configured float width does: `f64` under every width, `f32` under a
/// 32-bit width. Rendered documentation shows the 64-bit set.
///
/// ```
/// fn converts<T: beni::FromValue>() {}
/// converts::<f64>();
/// #[cfg(mrb_float32)]
/// converts::<f32>();
/// ```
///
/// An `f32` does not hold every value a 64-bit width carries:
///
/// ```compile_fail
/// fn converts<T: beni::FromValue>() {}
/// #[cfg(mrb_float32)]
/// compile_error!("a 32-bit float width fits an f32");
/// converts::<f32>();
/// ```
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

impl IntoValue for f32 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_float(mrb, sys::mrb_float::from(self))
    }
}

#[cfg(not(mrb_float32))]
impl IntoValue for f64 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        Value::from_float(mrb, sys::mrb_float::from(self))
    }
}

impl IntoValue for bool {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        if self {
            crate::value::qtrue().as_value()
        } else {
            crate::value::qfalse().as_value()
        }
    }
}

impl IntoValue for () {
    // A body with nothing to answer returns `()`, which Ruby reads as
    // `nil` — magnus's `IntoValue for ()`.
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        crate::value::qnil().as_value()
    }
}

// A handle on a Ruby object converts into the value naming that same
// object: the `Value`-newtype handles unwrap it, the class handles box
// their pointer.
#[cfg(feature = "bytes")]
impl IntoValue for bytes::Bytes {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        crate::ReprValue::as_value(mrb.str_new(&self))
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
    // established by the tag check it runs behind.
    (value.tag() == sys::MRB_TT_INTEGER).then(|| unsafe { value.unbox_integer() })
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
        // established by the tag check immediately before it.
        (value.tag() == sys::MRB_TT_FLOAT).then(|| unsafe { value.unbox_float() })
    }
}

#[cfg(mrb_float32)]
impl FromValue for f32 {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // The cfg admits only a configured width an `f32` holds every
        // value of, so narrowing the widened read loses nothing.
        <f64 as FromValue>::from_value(value).map(|f| f as f32)
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

impl FromValue for String {
    // A String-tagged value whose bytes are valid UTF-8 converts; a
    // non-string tag and a non-UTF-8 string both reject — a Rust
    // `String` is UTF-8 by invariant, so non-UTF-8 bytes genuinely
    // cannot become one.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Self::from_utf8(RString::from_value(value)?.copy_bytes()).ok()
    }
}

impl FromValue for Vec<u8> {
    // A String-tagged value yields its bytes verbatim — arbitrary, not
    // required to be UTF-8 — the binary counterpart to the owned
    // `String` conversion for callers that handle raw byte strings. A
    // non-string tag rejects.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        Some(RString::from_value(value)?.copy_bytes())
    }
}
