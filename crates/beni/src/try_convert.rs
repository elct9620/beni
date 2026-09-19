//! `TryConvert`, the conversion a method's arguments cross — magnus's
//! `TryConvert` in mruby's terms.
//!
//! A mismatch surfaces the exception mruby raises for the same mismatch,
//! worded as mruby words it. Where CRuby dispatches an implicit
//! conversion (`to_str`, `to_ary`, `to_proc`, `to_path`) mruby has none,
//! so a handle converts on its type tag alone.

use crate::{
    method::core_exception, Array, Error, ExceptionClass, FromValue, Hash, Mrb, Proc, RClass,
    RModule, RString, Range, Symbol, Value,
};
use core::num::{
    NonZeroI128, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8, NonZeroIsize, NonZeroU128,
    NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8, NonZeroUsize,
};

/// Convert an mruby `Value` into a Rust value or typed handle, or answer
/// the `Err` carrying the exception mruby raises for the mismatch. Mirrors
/// magnus's `TryConvert`, taking the interpreter the exception is built
/// in; the conversion a registered method's arguments, `scan_args`, and
/// `get_kwargs` apply.
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

fn exception(mrb: &Mrb, class: &core::ffi::CStr, msg: &str) -> Error {
    Error::Exception(core_exception(mrb, class, msg))
}

pub(crate) fn type_error(mrb: &Mrb, msg: &str) -> Error {
    exception(mrb, c"TypeError", msg)
}

pub(crate) fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    exception(mrb, c"ArgumentError", msg)
}

/// mruby's `%Y`: `nil`, `true`, or `false` itself, any other value its
/// class.
fn described(val: Value, mrb: &Mrb) -> String {
    if val.is_nil() || val.is_true() || val.is_false() {
        val.inspect(mrb)
    } else {
        val.classname(mrb)
    }
}

/// The `TypeError` mruby raises for a value its `target` class does not
/// convert.
pub(crate) fn not_convertible(val: Value, mrb: &Mrb, target: &str) -> Error {
    type_error(
        mrb,
        &format!("{} cannot be converted to {target}", described(val, mrb)),
    )
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

impl<T: TryConvert> TryConvert for Option<T> {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        (!val.is_nil())
            .then(|| T::try_convert(val, mrb))
            .transpose()
    }
}

macro_rules! try_convert_integer {
    ($($int:ty),* $(,)?) => {$(
        impl TryConvert for $int {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                let n = val.as_int(mrb)?;
                <$int>::try_from(n)
                    .map_err(|_| exception(mrb, c"RangeError", &format!("{n} out of range")))
            }
        }
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
        val.as_float(mrb)
    }
}

impl TryConvert for f32 {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        f64::try_convert(val, mrb).map(|f| f as f32)
    }
}

macro_rules! try_convert_tagged {
    ($($handle:ty => $target:literal),* $(,)?) => {$(
        impl TryConvert for $handle {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                <$handle>::from_value(val).ok_or_else(|| not_convertible(val, mrb, $target))
            }
        }
    )*};
}

try_convert_tagged!(
    RString => "String", Array => "Array", Hash => "Hash", Symbol => "Symbol", Range => "Range",
);

macro_rules! try_convert_class {
    ($($handle:ty => $kind:literal),* $(,)?) => {$(
        impl TryConvert for $handle {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                <$handle>::from_value(val).ok_or_else(|| {
                    type_error(mrb, &format!("{} is not {}", val.inspect(mrb), $kind))
                })
            }
        }
    )*};
}

try_convert_class!(
    RClass => "a class",
    RModule => "a module",
    ExceptionClass => "a class inheriting Exception",
);

impl TryConvert for Proc {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        Proc::from_value(val).ok_or_else(|| {
            type_error(
                mrb,
                &format!("wrong argument type {} (expected Proc)", val.classname(mrb)),
            )
        })
    }
}

impl TryConvert for String {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RString::try_convert(val, mrb)?.to_string(mrb)
    }
}

impl TryConvert for char {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        RString::try_convert(val, mrb)?.to_char(mrb)
    }
}

impl TryConvert for std::path::PathBuf {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let string = RString::try_convert(val, mrb)?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            Ok(std::ffi::OsString::from_vec(string.to_bytes()).into())
        }
        #[cfg(not(unix))]
        {
            string.to_string(mrb).map(Into::into)
        }
    }
}
