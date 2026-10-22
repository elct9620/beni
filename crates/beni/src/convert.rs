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
//! text, byte vectors, paths, `Option`, which reads `nil` as `None`, and
//! the Rust collections, which convert native elements only. A
//! typed handle's conversions sit beside the handle, and every conversion
//! copies rather than borrowing VM storage.

use crate::{sys, sys::AsRawValue, Mrb, RString, ReprValue, Value};

/// Box a Rust value into an mruby `Value`. Infallible — every
/// implementor has a total mapping into the value domain. Mirrors
/// magnus's `IntoValue`; the call shape is `n.into_value(mrb)`.
///
/// A Rust integer converts only where every value it holds fits the
/// archive's configured integer width, or where a bigint archive holds it
/// as an arbitrary-width Integer: `i8` / `i16` / `i32` / `u8` / `u16`
/// everywhere, `u32` / `i64` under a 64-bit width or a bigint archive,
/// `isize` wherever the target's pointer width fits the configured one or
/// the archive is a bigint archive, and `u64` / `usize` / `i128` / `u128`
/// only against a bigint archive. Rendered documentation shows the set of
/// a 64-bit bigint archive.
///
/// ```
/// fn converts<T: beni::IntoValue>() {}
/// converts::<i8>();
/// converts::<i16>();
/// converts::<i32>();
/// converts::<u8>();
/// converts::<u16>();
/// #[cfg(any(mrb_int64, mrb_bigint))]
/// {
///     converts::<u32>();
///     converts::<i64>();
///     converts::<isize>();
/// }
/// #[cfg(mrb_bigint)]
/// {
///     converts::<u64>();
///     converts::<usize>();
///     converts::<i128>();
///     converts::<u128>();
/// }
/// ```
///
/// A 32-bit width without a bigint archive does not hold every `i64`:
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// #[cfg(any(mrb_int64, mrb_bigint))]
/// compile_error!("a 64-bit width or a bigint archive holds every i64");
/// converts::<i64>();
/// ```
///
/// No width alone holds every `u64`, `usize`, `i128`, or `u128`:
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// #[cfg(mrb_bigint)]
/// compile_error!("a bigint archive holds every u64");
/// converts::<u64>();
/// ```
///
/// ```compile_fail
/// fn converts<T: beni::IntoValue>() {}
/// #[cfg(mrb_bigint)]
/// compile_error!("a bigint archive holds every i128");
/// converts::<i128>();
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

#[cfg(any(mrb_int64, mrb_bigint))]
impl IntoValue for i64 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.integer_from_i64(self).as_value()
    }
}

#[cfg(any(mrb_int64, mrb_bigint))]
impl IntoValue for u32 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.integer_from_i64(i64::from(self)).as_value()
    }
}

#[cfg(any(mrb_int64, mrb_bigint, not(target_pointer_width = "64")))]
impl IntoValue for isize {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        // Every cargo target's pointer width is at most 64 bits, so the
        // casts never truncate; without either 64-bit holder the cfg
        // admits only a pointer width the configured integer width holds.
        #[cfg(any(mrb_int64, mrb_bigint))]
        return mrb.integer_from_i64(self as i64).as_value();
        #[cfg(not(any(mrb_int64, mrb_bigint)))]
        return Value::from_int(mrb, self as sys::mrb_int);
    }
}

#[cfg(mrb_bigint)]
impl IntoValue for u64 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.integer_from_u64(self).as_value()
    }
}

#[cfg(mrb_bigint)]
impl IntoValue for usize {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        // Every cargo target's pointer width is at most 64 bits.
        mrb.integer_from_u64(self as u64).as_value()
    }
}

#[cfg(mrb_bigint)]
impl IntoValue for i128 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.integer_from_i128(self).as_value()
    }
}

#[cfg(mrb_bigint)]
impl IntoValue for u128 {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.integer_from_u128(self).as_value()
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

impl IntoValue for &str {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.str_new(self.as_bytes()).as_value()
    }
}

impl IntoValue for String {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.as_str().into_value(mrb)
    }
}

impl IntoValue for char {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.encode_utf8(&mut [0; 4]).into_value(mrb)
    }
}

impl IntoValue for &core::ffi::CStr {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        mrb.str_from_c_str(self).as_value()
    }
}

impl IntoValue for std::ffi::CString {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.as_c_str().into_value(mrb)
    }
}

impl IntoValue for &std::path::Path {
    /// The path's bytes on a Unix target. mruby has no encodings, so any
    /// other target takes the UTF-8 text magnus falls back to where it
    /// finds no UTF-16 encoding.
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            mrb.str_new(self.as_os_str().as_bytes()).as_value()
        }
        #[cfg(not(unix))]
        {
            self.to_string_lossy().as_ref().into_value(mrb)
        }
    }
}

impl IntoValue for std::path::PathBuf {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        self.as_path().into_value(mrb)
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    #[inline]
    fn into_value(self, mrb: &Mrb) -> Value {
        match self {
            Some(t) => t.into_value(mrb),
            None => crate::value::qnil().as_value(),
        }
    }
}

/// A Rust type whose values hold no `Value`, so a heap structure of them
/// converts into Ruby. Mirrors magnus's `IntoValueFromNative`: it bounds
/// the elements a `Vec` or a map converts, which no Rust heap structure
/// may carry as `Value`s.
///
/// ```compile_fail
/// fn native<T: beni::IntoValueFromNative>() {}
/// native::<Vec<beni::Value>>();
/// ```
///
/// # Safety
///
/// Implement only for a type whose values hold no `Value`, directly or
/// through any field.
pub unsafe trait IntoValueFromNative: IntoValue {}

macro_rules! into_value_from_native {
    ($($(#[$cfg:meta])* $t:ty),* $(,)?) => {$(
        $(#[$cfg])*
        unsafe impl IntoValueFromNative for $t {}
    )*};
}

into_value_from_native!(
    i8,
    i16,
    i32,
    u8,
    u16,
    f32,
    bool,
    (),
    &str,
    String,
    char,
    &core::ffi::CStr,
    std::ffi::CString,
    &std::path::Path,
    std::path::PathBuf,
    #[cfg(any(mrb_int64, mrb_bigint))]
    i64,
    #[cfg(any(mrb_int64, mrb_bigint))]
    u32,
    #[cfg(any(mrb_int64, mrb_bigint, not(target_pointer_width = "64")))]
    isize,
    #[cfg(mrb_bigint)]
    u64,
    #[cfg(mrb_bigint)]
    usize,
    #[cfg(mrb_bigint)]
    i128,
    #[cfg(mrb_bigint)]
    u128,
    #[cfg(not(mrb_float32))]
    f64,
);

unsafe impl<T: IntoValueFromNative> IntoValueFromNative for Option<T> {}

impl<T: IntoValueFromNative> IntoValue for Vec<T> {
    fn into_value(self, mrb: &Mrb) -> Value {
        let ary = mrb.ary_new_capa(self.len());
        for item in self {
            // The array holds each element once pushed, so the element's
            // own arena slots release with its scope.
            let _scope = mrb.arena_scope();
            let v = item.into_value(mrb);
            // SAFETY: `mrb` is alive and `ary` is the unfrozen Array just
            // built; the push raises nothing but the out-of-memory error
            // every allocation may.
            unsafe { sys::mrb_ary_push(mrb.as_ptr(), ary.as_value().as_raw(), v.as_raw()) };
        }
        ary.as_value()
    }
}

unsafe impl<T: IntoValueFromNative> IntoValueFromNative for Vec<T> {}

/// A new Hash of each pair, a pair whose key raises while the Hash stores
/// it left out as magnus's conversion leaves it.
fn hash_from_pairs<K, V>(mrb: &Mrb, len: usize, pairs: impl Iterator<Item = (K, V)>) -> Value
where
    K: IntoValueFromNative,
    V: IntoValueFromNative,
{
    let hash = mrb.hash_new_capa(len);
    for (k, v) in pairs {
        // The hash holds each pair once stored, so the pair's own arena
        // slots release with its scope.
        let _scope = mrb.arena_scope();
        let key = k.into_value(mrb);
        let val = v.into_value(mrb);
        let _ = mrb.protect_unit(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame and `hash`
            // is the unfrozen Hash just built; storing the key may
            // dispatch its `hash` or `eql?` and raise, caught by `protect`.
            unsafe {
                sys::mrb_hash_set(
                    mrb.as_ptr(),
                    hash.as_value().as_raw(),
                    key.as_raw(),
                    val.as_raw(),
                )
            };
        });
    }
    hash.as_value()
}

impl<K, V> IntoValue for std::collections::HashMap<K, V>
where
    K: IntoValueFromNative,
    V: IntoValueFromNative,
{
    fn into_value(self, mrb: &Mrb) -> Value {
        let len = self.len();
        hash_from_pairs(mrb, len, self.into_iter())
    }
}

unsafe impl<K, V> IntoValueFromNative for std::collections::HashMap<K, V>
where
    K: IntoValueFromNative,
    V: IntoValueFromNative,
{
}

impl<K, V> IntoValue for std::collections::BTreeMap<K, V>
where
    K: IntoValueFromNative,
    V: IntoValueFromNative,
{
    fn into_value(self, mrb: &Mrb) -> Value {
        let len = self.len();
        hash_from_pairs(mrb, len, self.into_iter())
    }
}

unsafe impl<K, V> IntoValueFromNative for std::collections::BTreeMap<K, V>
where
    K: IntoValueFromNative,
    V: IntoValueFromNative,
{
}

macro_rules! into_value_tuple {
    ($($t:ident $i:tt),+) => {
        impl<$($t: IntoValue),+> IntoValue for ($($t,)+) {
            fn into_value(self, mrb: &Mrb) -> Value {
                mrb.ary_new_from_values(&[$(self.$i.into_value(mrb)),+]).as_value()
            }
        }

        unsafe impl<$($t: IntoValueFromNative),+> IntoValueFromNative for ($($t,)+) {}
    };
}

into_value_tuple!(T0 0);
into_value_tuple!(T0 0, T1 1);
into_value_tuple!(T0 0, T1 1, T2 2);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9, T10 10);
into_value_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9, T10 10, T11 11);

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
