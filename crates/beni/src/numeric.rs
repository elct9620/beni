//! The handle on any Numeric, mirroring magnus's `NumericValue`.

use crate::{Error, Mrb, RClass, ReprValue, TryConvert, Value};
use beni_sys as sys;

/// A value whose class descends from `Numeric`: an Integer, a Float, a
/// Rational, a Complex, or an instance of a Ruby subclass. Mirrors
/// magnus's `NumericValue`, converting through `TryConvert` alone, since
/// telling a Numeric needs the class the interpreter holds.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct NumericValue(Value);

impl NumericValue {
    /// Wrap a `Value` the caller has established descends from `Numeric`.
    ///
    /// # Safety
    ///
    /// `v`'s class must descend from `Numeric`.
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }
}

crate::value::value_backed_repr!(NumericValue);

impl TryConvert for NumericValue {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        // `Numeric` is the superclass `mrb_open` gives `Integer`, read
        // without a constant lookup a program could redirect.
        // SAFETY: `class_integer` is a live class handle; the shim only
        // follows its `super` chain, which the class keeps reachable.
        let numeric = RClass::from_raw_unchecked(unsafe {
            sys::mrb_class_superclass_func(mrb.class_integer().as_internal())
        });
        if val.is_kind_of(mrb, numeric) {
            // SAFETY: the kind check just held.
            Ok(unsafe { Self::from_value_unchecked(val) })
        } else {
            Err(crate::try_convert::not_convertible(val, mrb, "Numeric"))
        }
    }
}
