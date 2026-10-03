//! Handles a value converts into by its type tag alone.
//!
//! Each handle here carries no operations of its own: it exists so the
//! `FromValue` downcast answers "what type is this?" for every tag a
//! typed caller can hold, as magnus's `Qnil`, `Integer`, `RObject`, … do.

use crate::{
    convert::{FromValue, IntoValue},
    sys::AsRawValue,
    value::{private, ReprValue},
    Mrb, Value,
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
}

tagged_handle!(
    /// The `nil` value. Mirrors magnus's `value::Qnil`.
    Qnil,
    |value| value.is_nil()
);

tagged_handle!(
    /// The `true` value. Mirrors magnus's `value::Qtrue`.
    Qtrue,
    |value| value.tag() == sys::MRB_TT_TRUE
);

tagged_handle!(
    /// The `false` value, told apart from `nil`, which shares its tag.
    /// Mirrors magnus's `value::Qfalse`.
    Qfalse,
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
    Exception,
    |value| value.tag() == sys::MRB_TT_EXCEPTION
);

tagged_handle!(
    /// An ordinary object — what `Object.new` allocates. Mirrors
    /// magnus's `RObject`.
    RObject,
    |value| value.tag() == sys::MRB_TT_OBJECT
);

tagged_handle!(
    /// A Fiber, from the `mruby-fiber` gem. Mirrors magnus's `Fiber`.
    Fiber,
    |value| value.tag() == sys::MRB_TT_FIBER
);

tagged_handle!(
    /// A Struct instance, from the `mruby-struct` gem. Mirrors magnus's
    /// `RStruct`.
    RStruct,
    |value| value.tag() == sys::MRB_TT_STRUCT
);

tagged_handle!(
    /// A Set, from the `mruby-set` gem.
    RSet,
    |value| value.tag() == sys::MRB_TT_SET
);

tagged_handle!(
    /// A Rational, from the `mruby-rational` gem. Mirrors magnus's
    /// `RRational`.
    RRational,
    |value| value.tag() == sys::MRB_TT_RATIONAL
);

tagged_handle!(
    /// A Complex, from the `mruby-complex` gem. Mirrors magnus's
    /// `RComplex`.
    RComplex,
    |value| value.tag() == sys::MRB_TT_COMPLEX
);

tagged_handle!(
    /// An inline struct of any type — the untyped counterpart of
    /// `Inline<T>`, as `RTypedData` is of `Obj<T>`.
    RInlineStruct,
    |value| value.tag() == sys::MRB_TT_ISTRUCT
);

tagged_handle!(
    /// A bare C pointer a C extension boxed.
    RCptr,
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
