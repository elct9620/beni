//! `TryConvert`, the conversion a method's receiver and arguments cross —
//! magnus's `TryConvert` in mruby's terms.
//!
//! A mismatch surfaces the exception mruby raises for the same mismatch,
//! worded as mruby words it. Where CRuby dispatches an implicit
//! conversion (`to_str`, `to_ary`, `to_proc`, `to_path`) mruby has none,
//! so a handle converts on its type tag alone.

use crate::{error::core_error, Error, Mrb, RArray, RHash, RString, ReprValue, Value};
use core::num::{
    NonZeroI128, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8, NonZeroIsize, NonZeroU128,
    NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8, NonZeroUsize,
};

/// Convert an mruby `Value` into a Rust value or typed handle, or answer
/// the `Err` carrying the exception mruby raises for the mismatch. Mirrors
/// magnus's `TryConvert`, taking the interpreter the exception is built
/// in; the conversion a registered method's receiver and arguments,
/// `scan_args`, and `get_kwargs` apply.
///
/// Unlike the `FromValue` downcast, a numeric target converts across the
/// numeric types as mruby's own C-method arguments do, and an `f32`
/// converts under every configured float width, narrowing to the
/// nearest `f32`:
///
/// ```
/// fn converts<T: beni::TryConvert>() {}
/// converts::<f32>();
/// converts::<f64>();
/// ```
pub trait TryConvert: Sized {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error>;
}

/// A `TryConvert` target whose values hold no `Value`, so a Rust heap
/// structure may store them past the arena. Mirrors magnus's
/// `TryConvertOwned`: it bounds the targets a `Vec` or a map converts its
/// elements to, which keep each element reachable only while it
/// converts.
///
/// A fixed-length array is owned only when its elements are, where
/// magnus marks `[T; N]` owned for any `T`: an owned conversion releases
/// each element's arena hold once it converts, so a `Value` inside an
/// array element would go unprotected.
///
/// ```compile_fail
/// fn owned<T: beni::TryConvertOwned>() {}
/// owned::<Vec<beni::Value>>();
/// ```
///
/// ```compile_fail
/// fn owned<T: beni::TryConvertOwned>() {}
/// owned::<[beni::RString; 2]>();
/// ```
///
/// # Safety
///
/// Implement only for a type whose values hold no `Value`, directly or
/// through any field.
pub unsafe trait TryConvertOwned: TryConvert {}

pub(crate) fn type_error(mrb: &Mrb, msg: &str) -> Error {
    core_error(mrb, c"TypeError", msg)
}

pub(crate) fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    core_error(mrb, c"ArgumentError", msg)
}

/// mruby's `%Y`: `nil`, `true`, or `false` itself, any other value its
/// class.
fn described(val: Value, mrb: &Mrb) -> String {
    // `nil` and `false` share the false tag.
    if matches!(val.tag(), beni_sys::MRB_TT_FALSE | beni_sys::MRB_TT_TRUE) {
        val.inspect(mrb)
    } else {
        val.classname(mrb)
    }
}

/// The `TypeError` mruby's `mrb_check_type` raises for a value of the
/// wrong type, naming the value as that check names it.
pub(crate) fn wrong_argument_type(val: Value, mrb: &Mrb, expected: &str) -> Error {
    // SAFETY: `mrb_immediate_p` is a pure predicate over the value word.
    let immediate = unsafe { beni_sys::mrb_immediate_p_func(val.0) };
    let named = if val.is_nil() {
        "nil".to_owned()
    } else if val.tag() == beni_sys::MRB_TT_INTEGER {
        "Integer".to_owned()
    } else if val.tag() == beni_sys::MRB_TT_SYMBOL {
        "Symbol".to_owned()
    } else if immediate {
        val.to_string(mrb)
    } else {
        val.classname(mrb)
    };
    type_error(
        mrb,
        &format!("wrong argument type {named} (expected {expected})"),
    )
}

/// The `TypeError` mruby raises for a value its `target` class does not
/// convert.
pub(crate) fn not_convertible(val: Value, mrb: &Mrb, target: &str) -> Error {
    type_error(
        mrb,
        &format!("{} cannot be converted to {target}", described(val, mrb)),
    )
}

/// The `TypeError` for an array whose length a fixed-length target
/// does not match.
pub(crate) fn length_error(mrb: &Mrb, len: usize) -> Error {
    type_error(mrb, &format!("expected Array of length {len}"))
}

pub(crate) fn invalid_utf8(mrb: &Mrb) -> Error {
    argument_error(mrb, "invalid UTF-8 byte sequence")
}

impl TryConvert for Value {
    #[inline]
    fn try_convert(val: Value, _mrb: &Mrb) -> Result<Self, Error> {
        Ok(val)
    }
}

impl TryConvert for bool {
    #[inline]
    fn try_convert(val: Value, _mrb: &Mrb) -> Result<Self, Error> {
        Ok(val.to_bool())
    }
}

unsafe impl TryConvertOwned for bool {}

impl<T: TryConvert> TryConvert for Option<T> {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        (!val.is_nil())
            .then(|| T::try_convert(val, mrb))
            .transpose()
    }
}

unsafe impl<T: TryConvertOwned> TryConvertOwned for Option<T> {}

macro_rules! try_convert_integer {
    ($($int:ty),* $(,)?) => {$(
        impl TryConvert for $int {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                let n = crate::Integer::try_convert(val, mrb)?.to_i64(mrb)?;
                <$int>::try_from(n)
                    .map_err(|_| core_error(mrb, c"RangeError", &format!("{n} out of range")))
            }
        }

        unsafe impl TryConvertOwned for $int {}
    )*};
}

try_convert_integer!(i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, isize, usize);

macro_rules! try_convert_non_zero {
    ($($non_zero:ty => $int:ty),* $(,)?) => {$(
        impl TryConvert for $non_zero {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                <$non_zero>::new(<$int>::try_convert(val, mrb)?)
                    .ok_or_else(|| argument_error(mrb, "value must be non-zero"))
            }
        }

        unsafe impl TryConvertOwned for $non_zero {}
    )*};
}

try_convert_non_zero!(
    NonZeroI8 => i8, NonZeroI16 => i16, NonZeroI32 => i32, NonZeroI64 => i64,
    NonZeroI128 => i128, NonZeroIsize => isize, NonZeroU8 => u8, NonZeroU16 => u16,
    NonZeroU32 => u32, NonZeroU64 => u64, NonZeroU128 => u128, NonZeroUsize => usize,
);

impl TryConvert for f64 {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        crate::Float::try_convert(val, mrb).map(crate::Float::to_f64)
    }
}

unsafe impl TryConvertOwned for f64 {}

impl TryConvert for f32 {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        f64::try_convert(val, mrb).map(|f| f as f32)
    }
}

/// The `TryConvert` of a handle that converts on its tag alone, naming
/// `$target` in mruby's conversion `TypeError`.
macro_rules! try_convert_tagged {
    ($handle:ty => $target:literal) => {
        impl $crate::TryConvert for $handle {
            #[inline]
            fn try_convert(val: $crate::Value, mrb: &$crate::Mrb) -> Result<Self, $crate::Error> {
                <$handle as $crate::FromValue>::from_value(val)
                    .ok_or_else(|| $crate::try_convert::not_convertible(val, mrb, $target))
            }
        }
    };
}
pub(crate) use try_convert_tagged;

unsafe impl TryConvertOwned for f32 {}

impl TryConvert for String {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RString::try_convert(val, mrb)?.to_string(mrb)
    }
}

unsafe impl TryConvertOwned for String {}

#[cfg(feature = "bytes")]
impl TryConvert for bytes::Bytes {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        Ok(RString::try_convert(val, mrb)?.copy_bytes().into())
    }
}

#[cfg(feature = "bytes")]
unsafe impl TryConvertOwned for bytes::Bytes {}

impl TryConvert for char {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RString::try_convert(val, mrb)?.to_char(mrb)
    }
}

unsafe impl TryConvertOwned for char {}

impl TryConvert for std::path::PathBuf {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let string = RString::try_convert(val, mrb)?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            Ok(std::ffi::OsString::from_vec(string.copy_bytes()).into())
        }
        #[cfg(not(unix))]
        {
            string.to_string(mrb).map(Into::into)
        }
    }
}

unsafe impl TryConvertOwned for std::path::PathBuf {}

impl<T: TryConvertOwned> TryConvert for Vec<T> {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RArray::try_convert(val, mrb)?.to_vec(mrb)
    }
}

unsafe impl<T: TryConvertOwned> TryConvertOwned for Vec<T> {}

impl<T: TryConvert, const N: usize> TryConvert for [T; N] {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RArray::try_convert(val, mrb)?.to_array(mrb)
    }
}

unsafe impl<T: TryConvertOwned, const N: usize> TryConvertOwned for [T; N] {}

macro_rules! try_convert_tuple {
    ($len:literal; $($t:ident $i:tt),+) => {
        impl<$($t: TryConvert),+> TryConvert for ($($t,)+) {
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                let ary = RArray::try_convert(val, mrb)?;
                if ary.len() != $len {
                    return Err(length_error(mrb, $len));
                }
                Ok(($($t::try_convert(ary.entry(mrb, $i), mrb)?,)+))
            }
        }

        unsafe impl<$($t: TryConvertOwned),+> TryConvertOwned for ($($t,)+) {}
    };
}

try_convert_tuple!(1; T0 0);
try_convert_tuple!(2; T0 0, T1 1);
try_convert_tuple!(3; T0 0, T1 1, T2 2);
try_convert_tuple!(4; T0 0, T1 1, T2 2, T3 3);
try_convert_tuple!(5; T0 0, T1 1, T2 2, T3 3, T4 4);
try_convert_tuple!(6; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
try_convert_tuple!(7; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
try_convert_tuple!(8; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);
try_convert_tuple!(9; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8);
try_convert_tuple!(10; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9);
try_convert_tuple!(11; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9, T10 10);
try_convert_tuple!(12; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8, T9 9, T10 10, T11 11);

impl<K, V> TryConvert for std::collections::HashMap<K, V>
where
    K: TryConvertOwned + Eq + core::hash::Hash,
    V: TryConvertOwned,
{
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RHash::try_convert(val, mrb)?.to_hash_map(mrb)
    }
}

unsafe impl<K, V> TryConvertOwned for std::collections::HashMap<K, V>
where
    K: TryConvertOwned + Eq + core::hash::Hash,
    V: TryConvertOwned,
{
}

impl<K, V> TryConvert for std::collections::BTreeMap<K, V>
where
    K: TryConvertOwned + Ord,
    V: TryConvertOwned,
{
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RHash::try_convert(val, mrb)?.to_btree_map(mrb)
    }
}

unsafe impl<K, V> TryConvertOwned for std::collections::BTreeMap<K, V>
where
    K: TryConvertOwned + Ord,
    V: TryConvertOwned,
{
}
