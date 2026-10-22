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
    Error, Mrb, TryConvert, Value,
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

// SAFETY: `nil`, `true`, and `false` are immediate under every boxing
// mruby builds, so their values hold nothing the collector can free.
unsafe impl crate::TryConvertOwned for Qnil {}
// SAFETY: as `Qnil`.
unsafe impl crate::TryConvertOwned for Qtrue {}
// SAFETY: as `Qnil`.
unsafe impl crate::TryConvertOwned for Qfalse {}

tagged_handle!(
    /// An Integer, of the fixed-width or the arbitrary-width tag.
    /// Mirrors magnus's `Integer`.
    Integer,
    |value| matches!(value.tag(), sys::MRB_TT_INTEGER | sys::MRB_TT_BIGINT)
);

tagged_handle!(
    /// An Integer of the fixed-width tag. Mirrors magnus's `Fixnum`, but
    /// is no `TryConvertOwned`: word boxing allocates a value beyond the
    /// inline range as a collectable object of this same tag
    /// (`mrb_boxing_int_value`, `vendor/mruby/src/etc.c:458`).
    Fixnum,
    |value| value.tag() == sys::MRB_TT_INTEGER
);

#[cfg(mrb_bigint)]
tagged_handle!(
    /// An Integer of the arbitrary-width tag, which only a bigint archive
    /// allocates. Mirrors magnus's `RBignum`.
    RBignum,
    |value| value.tag() == sys::MRB_TT_BIGINT
);

tagged_handle!(
    /// A Float. Mirrors magnus's `Float`.
    Float,
    |value| value.tag() == sys::MRB_TT_FLOAT
);

tagged_handle!(
    /// A Float the value word carries itself. Mirrors magnus's `Flonum`:
    /// mruby's word boxing with inline floats carries most floats in the
    /// word and allocates the rest, as CRuby's flonums do, while
    /// no-boxing carries every float and word boxing without inline
    /// floats none (`vendor/mruby/include/mruby/boxing_word.h`).
    Flonum,
    // SAFETY: mrb_immediate_p is a pure read of the value word.
    |value| value.tag() == sys::MRB_TT_FLOAT && unsafe { sys::mrb_immediate_p_func(value.as_raw()) }
);

// SAFETY: a `Flonum` is immediate, holding nothing the collector can free.
unsafe impl crate::TryConvertOwned for Flonum {}

tagged_handle!(
    /// A Float the collector allocates. Mirrors magnus's `RFloat`.
    RFloat,
    // SAFETY: mrb_immediate_p is a pure read of the value word.
    |value| value.tag() == sys::MRB_TT_FLOAT && !unsafe { sys::mrb_immediate_p_func(value.as_raw()) }
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

/// The undefined value, which no Ruby code can name; `value::qundef`
/// returns it. Mirrors magnus's `value::Qundef`.
#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Qundef(pub(crate) Value);

impl Qundef {
    /// The undefined value as a `Value`. Mirrors magnus's
    /// `Qundef::as_value`.
    ///
    /// # Safety
    ///
    /// The value must never reach Ruby code — returned from a method,
    /// stored where a Ruby program reads, or passed to an operation
    /// that hands it on. Only a few places in mruby's API take it, such
    /// as an optional argument slot `mrb_get_args` leaves untouched.
    #[inline]
    pub unsafe fn as_value(self) -> Value {
        self.0
    }
}

impl FromValue for Qundef {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: mrb_undef_p is a pure read of the value and does not
        // touch `mrb_state`.
        unsafe { sys::mrb_undef_p_func(value.as_raw()) }.then_some(Self(value))
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

impl TryConvert for Flonum {
    /// Converts what `Float` converts, rejecting a Float the collector
    /// allocates with magnus's `RangeError`.
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let float = Float::try_convert(val, mrb)?;
        Self::from_value(float.as_value()).ok_or_else(|| {
            crate::error::core_error(mrb, c"RangeError", "float out of range for flonum")
        })
    }
}

impl TryConvert for RFloat {
    /// Converts what `Float` converts, rejecting a Float the value word
    /// carries with magnus's `RangeError`.
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let float = Float::try_convert(val, mrb)?;
        Self::from_value(float.as_value()).ok_or_else(|| {
            crate::error::core_error(mrb, c"RangeError", "float in range for flonum")
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
