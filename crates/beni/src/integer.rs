//! The Integer handles' conversions and reads, mirroring magnus's
//! `Integer`, `Fixnum`, and `RBignum`.
//!
//! An `Integer` is either tag; a `Fixnum` is the fixed-width one, whose
//! value the word carries, and an `RBignum` the arbitrary-width one a
//! bigint archive allocates once a value outgrows the configured integer
//! width.

#[cfg(mrb_bigint)]
use crate::RBignum;
use crate::{
    error::core_error, sys::AsRawValue, value::private::ReprValue as _, Error, Fixnum, FromValue,
    Integer, Mrb, RString, ReprValue, TryConvert, Value,
};
use beni_sys as sys;

impl TryConvert for Integer {
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        if let Some(int) = Self::from_value(val) {
            return Ok(int);
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; the coercion
            // raises the mismatch's `TypeError` or `RangeError`, caught by
            // `protect`, and otherwise answers an Integer.
            let v = Value::from_raw_unchecked(unsafe {
                sys::mrb_ensure_integer_type(mrb.as_ptr(), val.as_raw())
            });
            // SAFETY: the coercion answers an Integer of either tag.
            unsafe { Integer::from_value_unchecked(v) }
        })
    }
}

impl TryConvert for Fixnum {
    /// Converts what `Integer` converts, rejecting an Integer of the
    /// arbitrary-width tag with magnus's `RangeError`.
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let int = Integer::try_convert(val, mrb)?;
        Self::from_value(int.as_value())
            .ok_or_else(|| core_error(mrb, c"RangeError", "integer too big for fixnum"))
    }
}

#[cfg(mrb_bigint)]
impl TryConvert for RBignum {
    /// Converts what `Integer` converts, rejecting an Integer of the
    /// fixed-width tag with magnus's `RangeError`.
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        let int = Integer::try_convert(val, mrb)?;
        Self::from_value(int.as_value())
            .ok_or_else(|| core_error(mrb, c"RangeError", "integer too small for bignum"))
    }
}

impl Integer {
    /// The integer as an `i64`. Surfaces an `Err` carrying the
    /// `RangeError` mruby raises for an arbitrary-width Integer beyond the
    /// configured integer width. Mirrors magnus's `Integer::to_i64`.
    pub fn to_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        let fixed = if self.as_value().tag() == sys::MRB_TT_INTEGER {
            self.as_value()
        } else {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; narrowing
                // raises the `RangeError` for an Integer beyond the
                // configured width, caught by `protect`, and otherwise
                // answers a fixed-width Integer.
                Value::from_raw_unchecked(unsafe {
                    sys::mrb_ensure_int_type(mrb.as_ptr(), self.as_value().as_raw())
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
                sys::mrb_integer_to_str(
                    mrb.as_ptr(),
                    self.as_value().as_raw(),
                    base as sys::mrb_int,
                )
            });
            // SAFETY: a successful render answers a String.
            unsafe { RString::from_value_unchecked(v) }
        })
    }
}

#[cfg(mrb_bigint)]
impl RBignum {
    /// Whether the integer is positive. Mirrors magnus's
    /// `RBignum::is_positive`.
    pub fn is_positive(self, mrb: &Mrb) -> bool {
        // SAFETY: `self` carries the arbitrary-width tag `mrb_bint_sign`
        // reads, and `mrb` is alive by the `&Mrb` borrow; the read raises
        // nothing.
        unsafe { sys::mrb_bint_sign(mrb.as_ptr(), self.as_value().as_raw()) > 0 }
    }

    /// Whether the integer is negative. Mirrors magnus's
    /// `RBignum::is_negative`.
    pub fn is_negative(self, mrb: &Mrb) -> bool {
        !self.is_positive(mrb)
    }
}
