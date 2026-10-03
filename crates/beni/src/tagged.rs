//! Handles a value converts into by its type tag alone.
//!
//! Each handle exists so the `FromValue` downcast answers "what type is
//! this?" for every tag a typed caller can hold, and so a method can take
//! an argument of that type through `TryConvert`, as magnus's `Qnil`,
//! `Integer`, `RObject`, … do.

use crate::{
    convert::{FromValue, IntoValue},
    sys::AsRawValue,
    try_convert::wrong_argument_type,
    value::{private, ReprValue},
    Error, Mrb, RString, TryConvert, Value,
};
use beni_sys as sys;

macro_rules! tagged_handle {
    ($(#[$doc:meta])* $name:ident, |$value:ident| $accepts:expr) => {
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Copy, Clone)]
        pub struct $name(Value);

        impl FromValue for $name {
            #[inline]
            fn from_value($value: Value) -> Option<Self> {
                ($accepts).then_some(Self($value))
            }
        }

        impl IntoValue for $name {
            #[inline]
            fn into_value(self, _mrb: &Mrb) -> Value {
                self.0
            }
        }

        impl ReprValue for $name {
            #[inline]
            fn as_value(self) -> Value {
                self.0
            }
        }

        impl private::ReprValue for $name {
            #[inline]
            unsafe fn from_value_unchecked(v: Value) -> Self {
                Self(v)
            }
        }
    };
    ($(#[$doc:meta])* $name:ident => $expected:literal, |$value:ident| $accepts:expr) => {
        tagged_handle!($(#[$doc])* $name, |$value| $accepts);

        impl TryConvert for $name {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                Self::from_value(val).ok_or_else(|| wrong_argument_type(val, mrb, $expected))
            }
        }
    };
}

tagged_handle!(
    /// The `nil` value. Mirrors magnus's `value::Qnil`.
    Qnil => "NilClass",
    |value| value.is_nil()
);

tagged_handle!(
    /// The `true` value. Mirrors magnus's `value::Qtrue`.
    Qtrue => "TrueClass",
    |value| value.tag() == sys::MRB_TT_TRUE
);

tagged_handle!(
    /// The `false` value, told apart from `nil`, which shares its tag.
    /// Mirrors magnus's `value::Qfalse`.
    Qfalse => "FalseClass",
    // SAFETY: mrb_false_p is a pure read of the value and does not
    // touch `mrb_state`.
    |value| unsafe { sys::mrb_false_p_func(value.as_raw()) }
);

tagged_handle!(
    /// An Integer, of the fixed-width or the arbitrary-width tag.
    /// Mirrors magnus's `Integer`.
    Integer,
    |value| matches!(value.tag(), sys::MRB_TT_INTEGER | sys::MRB_TT_BIGINT)
);

tagged_handle!(
    /// A Float. Mirrors magnus's `Float`.
    Float,
    |value| value.tag() == sys::MRB_TT_FLOAT
);

tagged_handle!(
    /// An exception object. Mirrors magnus's `Exception`.
    Exception => "Exception",
    |value| value.tag() == sys::MRB_TT_EXCEPTION
);

tagged_handle!(
    /// An ordinary object — what `Object.new` allocates. Mirrors
    /// magnus's `RObject`.
    RObject => "Object",
    |value| value.tag() == sys::MRB_TT_OBJECT
);

tagged_handle!(
    /// A Fiber, from the `mruby-fiber` gem. Mirrors magnus's `Fiber`.
    Fiber => "Fiber",
    |value| value.tag() == sys::MRB_TT_FIBER
);

tagged_handle!(
    /// A Struct instance, from the `mruby-struct` gem. Mirrors magnus's
    /// `RStruct`.
    RStruct => "Struct",
    |value| value.tag() == sys::MRB_TT_STRUCT
);

tagged_handle!(
    /// A Set, from the `mruby-set` gem.
    RSet => "Set",
    |value| value.tag() == sys::MRB_TT_SET
);

tagged_handle!(
    /// A Rational, from the `mruby-rational` gem. Mirrors magnus's
    /// `RRational`.
    RRational => "Rational",
    |value| value.tag() == sys::MRB_TT_RATIONAL
);

tagged_handle!(
    /// A Complex, from the `mruby-complex` gem. Mirrors magnus's
    /// `RComplex`.
    RComplex => "Complex",
    |value| value.tag() == sys::MRB_TT_COMPLEX
);

tagged_handle!(
    /// An inline struct of any type — the untyped counterpart of
    /// `Inline<T>`, as `RTypedData` is of `Obj<T>`.
    RInlineStruct => "istruct",
    |value| value.tag() == sys::MRB_TT_ISTRUCT
);

tagged_handle!(
    /// A bare C pointer a C extension boxed.
    RCptr => "cptr",
    |value| value.tag() == sys::MRB_TT_CPTR
);

/// The undefined value, which no Ruby code can name. The handle never
/// converts back into a value. Mirrors magnus's `value::Qundef`.
#[derive(Copy, Clone)]
pub struct Qundef {
    _private: (),
}

impl FromValue for Qundef {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: mrb_undef_p is a pure read of the value and does not
        // touch `mrb_state`.
        unsafe { sys::mrb_undef_p_func(value.as_raw()) }.then_some(Self { _private: () })
    }
}

impl TryConvert for Integer {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        if let Some(int) = Self::from_value(val) {
            return Ok(int);
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; the coercion
            // raises the mismatch's `TypeError` or `RangeError`, caught by
            // `protect`, and otherwise answers an Integer.
            Integer(Value::from_raw_unchecked(unsafe {
                sys::mrb_ensure_integer_type(mrb.as_ptr(), val.as_raw())
            }))
        })
    }
}

impl TryConvert for Float {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        if let Some(float) = Self::from_value(val) {
            return Ok(float);
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; the coercion
            // raises the mismatch's `TypeError`, caught by `protect`, and
            // otherwise answers a Float.
            Float(Value::from_raw_unchecked(unsafe {
                sys::mrb_ensure_float_type(mrb.as_ptr(), val.as_raw())
            }))
        })
    }
}

impl Integer {
    /// The integer as an `i64`. Surfaces an `Err` carrying the
    /// `RangeError` mruby raises for an arbitrary-width Integer beyond the
    /// configured integer width. Mirrors magnus's `Integer::to_i64`.
    pub fn to_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        let fixed = if self.0.tag() == sys::MRB_TT_INTEGER {
            self.0
        } else {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; narrowing
                // raises the `RangeError` for an Integer beyond the
                // configured width, caught by `protect`, and otherwise
                // answers a fixed-width Integer.
                Value::from_raw_unchecked(unsafe {
                    sys::mrb_ensure_int_type(mrb.as_ptr(), self.0.as_raw())
                })
            })?
        };
        // SAFETY: `fixed` carries the fixed-width Integer tag.
        Ok(unsafe { fixed.unbox_integer() })
    }

    /// Render to a new `RString` in `base`, the way Ruby's
    /// `Integer#to_s(base)` does — `12345` to `"3039"` in base 16. Surfaces
    /// an `Err` carrying an `ArgumentError` for a `base` outside 2 through
    /// 36. Mirrors mruby's `mrb_integer_to_str`.
    pub fn to_r_string_radix(self, mrb: &Mrb, base: i32) -> Result<RString, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `self` is Integer-tagged by construction and `mrb` is
            // alive inside the protect frame; an invalid `base` raises the
            // `ArgumentError`, caught by `protect`.
            let v = Value::from_raw_unchecked(unsafe {
                sys::mrb_integer_to_str(mrb.as_ptr(), self.0.as_raw(), base as sys::mrb_int)
            });
            // SAFETY: a successful render answers a String.
            unsafe { RString::from_value_unchecked(v) }
        })
    }
}

impl Float {
    /// The float as an `f64`, which holds every configured float width.
    /// Mirrors magnus's `Float::to_f64`.
    pub fn to_f64(self) -> f64 {
        // SAFETY: a `Float` carries the Float tag.
        unsafe { self.0.unbox_float() }
    }

    /// The `Integer` this float truncates toward zero, the way Ruby's
    /// `Float#to_i` does — `3.9` to `3`, `-3.9` to `-3`. Surfaces an `Err`
    /// carrying a `RangeError` for an infinite or NaN float. Mirrors
    /// mruby's `mrb_float_to_integer`.
    pub fn to_integer(self, mrb: &Mrb) -> Result<Integer, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `self` is Float-tagged by construction and `mrb` is
            // alive inside the protect frame; an infinite or NaN float
            // raises the `RangeError`, caught by `protect`.
            Integer(Value::from_raw_unchecked(unsafe {
                sys::mrb_float_to_integer(mrb.as_ptr(), self.0.as_raw())
            }))
        })
    }
}
