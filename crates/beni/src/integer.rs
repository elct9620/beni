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

/// Which tag an `Integer` carries, so a read takes the path its tag needs.
enum IntegerType {
    Fixnum(Fixnum),
    #[cfg(mrb_bigint)]
    Bignum(RBignum),
}

impl Integer {
    fn integer_type(self) -> IntegerType {
        match Fixnum::from_value(self.as_value()) {
            Some(fix) => IntegerType::Fixnum(fix),
            // SAFETY: an Integer not of the fixed-width tag carries the
            // arbitrary-width one.
            #[cfg(mrb_bigint)]
            None => IntegerType::Bignum(unsafe { RBignum::from_value_unchecked(self.as_value()) }),
            #[cfg(not(mrb_bigint))]
            None => unreachable!("only a bigint archive allocates the arbitrary-width tag"),
        }
    }

    /// The integer as an `i8`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_i8`.
    pub fn to_i8(self, mrb: &Mrb) -> Result<i8, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_i8(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(_) => Err(bignum_out_of_range(mrb)),
        }
    }

    /// The integer as an `i16`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_i16`.
    pub fn to_i16(self, mrb: &Mrb) -> Result<i16, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_i16(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(_) => Err(bignum_out_of_range(mrb)),
        }
    }

    /// The integer as an `i32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_i32`.
    pub fn to_i32(self, mrb: &Mrb) -> Result<i32, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_i32(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_i32(mrb),
        }
    }

    /// The integer as an `i64`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_i64`.
    pub fn to_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        // Only the arbitrary-width read reaches the VM.
        #[cfg(not(mrb_bigint))]
        let _ = mrb;
        match self.integer_type() {
            IntegerType::Fixnum(fix) => Ok(fix.to_i64()),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_i64(mrb),
        }
    }

    /// The integer as an `i128`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_i128`.
    pub fn to_i128(self, mrb: &Mrb) -> Result<i128, Error> {
        // Only the arbitrary-width read reaches the VM.
        #[cfg(not(mrb_bigint))]
        let _ = mrb;
        match self.integer_type() {
            IntegerType::Fixnum(fix) => Ok(fix.to_i128()),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_i128(mrb),
        }
    }

    /// The integer as an `isize`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_isize`.
    pub fn to_isize(self, mrb: &Mrb) -> Result<isize, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_isize(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_isize(mrb),
        }
    }

    /// The integer as a `u8`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_u8`.
    pub fn to_u8(self, mrb: &Mrb) -> Result<u8, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_u8(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(_) => Err(bignum_out_of_range(mrb)),
        }
    }

    /// The integer as a `u16`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_u16`.
    pub fn to_u16(self, mrb: &Mrb) -> Result<u16, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_u16(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(_) => Err(bignum_out_of_range(mrb)),
        }
    }

    /// The integer as a `u32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_u32`.
    pub fn to_u32(self, mrb: &Mrb) -> Result<u32, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_u32(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_u32(mrb),
        }
    }

    /// The integer as a `u64`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_u64`.
    pub fn to_u64(self, mrb: &Mrb) -> Result<u64, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_u64(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_u64(mrb),
        }
    }

    /// The integer as a `u128`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_u128`.
    pub fn to_u128(self, mrb: &Mrb) -> Result<u128, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_u128(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_u128(mrb),
        }
    }

    /// The integer as a `usize`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Integer::to_usize`.
    pub fn to_usize(self, mrb: &Mrb) -> Result<usize, Error> {
        match self.integer_type() {
            IntegerType::Fixnum(fix) => fix.to_usize(mrb),
            #[cfg(mrb_bigint)]
            IntegerType::Bignum(big) => big.to_usize(mrb),
        }
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

/// The `RangeError` a fixed-width Integer outside a Rust target's range
/// surfaces, naming the value in its inspected form.
fn fixnum_out_of_range(mrb: &Mrb, n: i64) -> Error {
    core_error(mrb, c"RangeError", &format!("{n} out of range"))
}

/// The `RangeError` an arbitrary-width Integer outside a Rust target's
/// range surfaces, as mruby's own conversions word it.
#[cfg(mrb_bigint)]
fn bignum_out_of_range(mrb: &Mrb) -> Error {
    core_error(mrb, c"RangeError", "integer out of range")
}

impl Fixnum {
    /// The integer as an `i64`, which holds every configured integer
    /// width. Mirrors magnus's `Fixnum::to_i64`.
    pub fn to_i64(self) -> i64 {
        // SAFETY: a `Fixnum` carries the fixed-width tag.
        unsafe { self.as_value().unbox_integer() }
    }

    /// The integer as an `i128`. Mirrors magnus's `Fixnum::to_i128`.
    pub fn to_i128(self) -> i128 {
        i128::from(self.to_i64())
    }

    /// The integer as an `i8`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_i8`.
    pub fn to_i8(self, mrb: &Mrb) -> Result<i8, Error> {
        let n = self.to_i64();
        i8::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as an `i16`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_i16`.
    pub fn to_i16(self, mrb: &Mrb) -> Result<i16, Error> {
        let n = self.to_i64();
        i16::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as an `i32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_i32`.
    pub fn to_i32(self, mrb: &Mrb) -> Result<i32, Error> {
        let n = self.to_i64();
        i32::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as an `isize`, surfacing an `Err` carrying a
    /// `RangeError` outside its range. magnus's `Fixnum::to_isize` cannot
    /// fail, but a 64-bit configured integer width on a 32-bit target holds
    /// values no `isize` does.
    pub fn to_isize(self, mrb: &Mrb) -> Result<isize, Error> {
        let n = self.to_i64();
        isize::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `u8`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_u8`.
    pub fn to_u8(self, mrb: &Mrb) -> Result<u8, Error> {
        let n = self.to_i64();
        u8::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `u16`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_u16`.
    pub fn to_u16(self, mrb: &Mrb) -> Result<u16, Error> {
        let n = self.to_i64();
        u16::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `u32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_u32`.
    pub fn to_u32(self, mrb: &Mrb) -> Result<u32, Error> {
        let n = self.to_i64();
        u32::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `u64`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_u64`.
    pub fn to_u64(self, mrb: &Mrb) -> Result<u64, Error> {
        let n = self.to_i64();
        u64::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `u128`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_u128`.
    pub fn to_u128(self, mrb: &Mrb) -> Result<u128, Error> {
        let n = self.to_i64();
        u128::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }

    /// The integer as a `usize`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `Fixnum::to_usize`.
    pub fn to_usize(self, mrb: &Mrb) -> Result<usize, Error> {
        let n = self.to_i64();
        usize::try_from(n).map_err(|_| fixnum_out_of_range(mrb, n))
    }
}

#[cfg(mrb_bigint)]
impl RBignum {
    /// The integer as an `i64`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_i64`.
    pub fn to_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        // mruby's 64-bit read bounds a negative magnitude by the positive
        // maximum (`mpz_get_int`, `mrb_bint_as_int64`), refusing −2⁶³, so a
        // value it refuses is read again through its text.
        self.native_i64(mrb).or_else(|_| {
            let n = self.to_i128(mrb)?;
            i64::try_from(n).map_err(|_| bignum_out_of_range(mrb))
        })
    }

    #[cfg(mrb_int64)]
    fn native_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; narrowing
            // raises the `RangeError` for an Integer beyond the configured
            // width, caught by `protect`, and otherwise answers a
            // fixed-width Integer.
            let v = Value::from_raw_unchecked(unsafe {
                sys::mrb_ensure_int_type(mrb.as_ptr(), self.as_value().as_raw())
            });
            // SAFETY: the narrowing answers the fixed-width tag.
            unsafe { Fixnum::from_value_unchecked(v) }
        })
        .map(Fixnum::to_i64)
    }

    #[cfg(not(mrb_int64))]
    fn native_i64(self, mrb: &Mrb) -> Result<i64, Error> {
        let mut n = 0;
        mrb.protect_unit(|mrb| {
            // SAFETY: `self` carries the arbitrary-width tag the read takes
            // and `mrb` is alive inside the protect frame; a value beyond
            // `i64` raises the `RangeError`, caught by `protect`.
            n = unsafe { sys::mrb_bint_as_int64(mrb.as_ptr(), self.as_value().as_raw()) };
        })?;
        Ok(n)
    }

    /// The integer as a `u64`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_u64`.
    pub fn to_u64(self, mrb: &Mrb) -> Result<u64, Error> {
        let mut n = 0;
        mrb.protect_unit(|mrb| {
            // SAFETY: `self` carries the arbitrary-width tag the read takes
            // and `mrb` is alive inside the protect frame; a negative value
            // or one beyond `u64` raises the `RangeError`, caught by
            // `protect`.
            n = unsafe { sys::mrb_bint_as_uint64(mrb.as_ptr(), self.as_value().as_raw()) };
        })?;
        Ok(n)
    }

    /// The integer as an `i128`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_i128`.
    pub fn to_i128(self, mrb: &Mrb) -> Result<i128, Error> {
        let digits = self.hex(mrb)?;
        i128::from_str_radix(&digits, 16).map_err(|_| bignum_out_of_range(mrb))
    }

    /// The integer as a `u128`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_u128`.
    pub fn to_u128(self, mrb: &Mrb) -> Result<u128, Error> {
        let digits = self.hex(mrb)?;
        u128::from_str_radix(&digits, 16).map_err(|_| bignum_out_of_range(mrb))
    }

    /// The integer's base-16 text, signed as Ruby's `Integer#to_s(16)`
    /// signs it — the one form mruby publishes for a value wider than 64
    /// bits.
    fn hex(self, mrb: &Mrb) -> Result<String, Error> {
        self.as_integer().to_r_string_radix(mrb, 16)?.to_string(mrb)
    }

    /// The integer as an `i32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_i32`.
    pub fn to_i32(self, mrb: &Mrb) -> Result<i32, Error> {
        let n = self.to_i64(mrb)?;
        i32::try_from(n).map_err(|_| bignum_out_of_range(mrb))
    }

    /// The integer as an `isize`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_isize`.
    pub fn to_isize(self, mrb: &Mrb) -> Result<isize, Error> {
        let n = self.to_i64(mrb)?;
        isize::try_from(n).map_err(|_| bignum_out_of_range(mrb))
    }

    /// The integer as a `u32`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_u32`.
    pub fn to_u32(self, mrb: &Mrb) -> Result<u32, Error> {
        let n = self.to_u64(mrb)?;
        u32::try_from(n).map_err(|_| bignum_out_of_range(mrb))
    }

    /// The integer as a `usize`, surfacing an `Err` carrying a `RangeError`
    /// outside its range. Mirrors magnus's `RBignum::to_usize`.
    pub fn to_usize(self, mrb: &Mrb) -> Result<usize, Error> {
        let n = self.to_u64(mrb)?;
        usize::try_from(n).map_err(|_| bignum_out_of_range(mrb))
    }

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

impl Mrb {
    /// An `Integer` holding `n`, of the fixed-width tag where the
    /// configured integer width holds it and the arbitrary-width tag
    /// otherwise. Mirrors magnus's `Ruby::integer_from_i64`.
    #[cfg(any(mrb_int64, mrb_bigint))]
    pub fn integer_from_i64(&self, n: i64) -> Integer {
        let v = match sys::mrb_int::try_from(n) {
            Ok(fixed) => Value::from_int(self, fixed),
            #[cfg(mrb_int64)]
            Err(never) => match never {},
            #[cfg(not(mrb_int64))]
            Err(_) => self.bignum_from_i64_unchecked(n),
        };
        // SAFETY: either construction answers an Integer.
        unsafe { Integer::from_value_unchecked(v) }
    }

    /// An `Integer` holding `n`, of the fixed-width tag where the
    /// configured integer width holds it and the arbitrary-width tag
    /// otherwise. Mirrors magnus's `Ruby::integer_from_u64`.
    #[cfg(mrb_bigint)]
    pub fn integer_from_u64(&self, n: u64) -> Integer {
        match self.fixnum_from_u64(n) {
            Ok(fix) => fix.as_integer(),
            Err(big) => big.as_integer(),
        }
    }

    /// An `Integer` holding `n`, of the fixed-width tag where the
    /// configured integer width holds it and the arbitrary-width tag
    /// otherwise. Mirrors magnus's `Ruby::integer_from_i128`.
    #[cfg(mrb_bigint)]
    pub fn integer_from_i128(&self, n: i128) -> Integer {
        match i64::try_from(n) {
            Ok(n) => self.integer_from_i64(n),
            Err(_) => self.integer_from_hex(n < 0, n.unsigned_abs()),
        }
    }

    /// An `Integer` holding `n`, of the fixed-width tag where the
    /// configured integer width holds it and the arbitrary-width tag
    /// otherwise. Mirrors magnus's `Ruby::integer_from_u128`.
    #[cfg(mrb_bigint)]
    pub fn integer_from_u128(&self, n: u128) -> Integer {
        match u64::try_from(n) {
            Ok(n) => self.integer_from_u64(n),
            Err(_) => self.integer_from_hex(false, n),
        }
    }

    /// The `Fixnum` holding `n` where the configured integer width holds
    /// it, otherwise an `Err` carrying the `RBignum`. Mirrors magnus's
    /// `Ruby::fixnum_from_i64`.
    #[cfg(mrb_bigint)]
    pub fn fixnum_from_i64(&self, n: i64) -> Result<Fixnum, RBignum> {
        match sys::mrb_int::try_from(n) {
            // SAFETY: a value the width holds boxes into the fixed-width tag.
            Ok(fixed) => Ok(unsafe { Fixnum::from_value_unchecked(Value::from_int(self, fixed)) }),
            #[cfg(mrb_int64)]
            Err(never) => match never {},
            // SAFETY: a value past the width boxes into the arbitrary-width
            // tag.
            #[cfg(not(mrb_int64))]
            Err(_) => {
                Err(unsafe { RBignum::from_value_unchecked(self.bignum_from_i64_unchecked(n)) })
            }
        }
    }

    /// The `Fixnum` holding `n` where the configured integer width holds
    /// it, otherwise an `Err` carrying the `RBignum`. Mirrors magnus's
    /// `Ruby::fixnum_from_u64`.
    #[cfg(mrb_bigint)]
    pub fn fixnum_from_u64(&self, n: u64) -> Result<Fixnum, RBignum> {
        match sys::mrb_int::try_from(n) {
            // SAFETY: a value the width holds boxes into the fixed-width tag.
            Ok(fixed) => Ok(unsafe { Fixnum::from_value_unchecked(Value::from_int(self, fixed)) }),
            Err(_) => {
                // SAFETY: `self` is alive by the `&self` borrow; mruby
                // allocates the arbitrary-width Integer, raising nothing
                // but the out-of-memory error every allocation may.
                let v = Value::from_raw_unchecked(unsafe {
                    sys::mrb_bint_new_uint64(self.as_ptr(), n)
                });
                // SAFETY: a value past the width boxes into the
                // arbitrary-width tag.
                Err(unsafe { RBignum::from_value_unchecked(v) })
            }
        }
    }

    /// The `RBignum` holding `n` where the configured integer width does
    /// not hold it, otherwise an `Err` carrying the `Fixnum`. Mirrors
    /// magnus's `Ruby::bignum_from_i64`.
    #[cfg(mrb_bigint)]
    pub fn bignum_from_i64(&self, n: i64) -> Result<RBignum, Fixnum> {
        match self.fixnum_from_i64(n) {
            Ok(fix) => Err(fix),
            Err(big) => Ok(big),
        }
    }

    /// The `RBignum` holding `n` where the configured integer width does
    /// not hold it, otherwise an `Err` carrying the `Fixnum`. Mirrors
    /// magnus's `Ruby::bignum_from_u64`.
    #[cfg(mrb_bigint)]
    pub fn bignum_from_u64(&self, n: u64) -> Result<RBignum, Fixnum> {
        match self.fixnum_from_u64(n) {
            Ok(fix) => Err(fix),
            Err(big) => Ok(big),
        }
    }

    /// The arbitrary-width Integer holding `n`, which a 32-bit configured
    /// integer width does not.
    #[cfg(all(mrb_bigint, not(mrb_int64)))]
    fn bignum_from_i64_unchecked(&self, n: i64) -> Value {
        // mruby's signed 64-bit construction fills a limb buffer it never
        // initialized (`mrb_bint_new_int64`), so a negative value is built
        // from its text and a positive one through the unsigned call.
        match u64::try_from(n) {
            Ok(n) => match self.fixnum_from_u64(n) {
                Ok(fix) => fix.as_value(),
                Err(big) => big.as_value(),
            },
            Err(_) => self
                .integer_from_hex(true, u128::from(n.unsigned_abs()))
                .as_value(),
        }
    }

    /// The Integer whose base-16 text is `magnitude`, negated when
    /// `negative` — the one form mruby publishes for building a value
    /// wider than 64 bits.
    #[cfg(mrb_bigint)]
    fn integer_from_hex(&self, negative: bool, magnitude: u128) -> Integer {
        let text = if negative {
            format!("-{magnitude:x}")
        } else {
            format!("{magnitude:x}")
        };
        let digits = self.str_new(text.as_bytes());
        // SAFETY: `self` is alive by the `&self` borrow and `digits` is a
        // String. The lenient parse raises only on a radix outside 2
        // through 36, and the text is clean base 16; the strict parse is
        // avoided because it refuses a bigint's digits as trailing bytes.
        let v = Value::from_raw_unchecked(unsafe {
            sys::mrb_str_to_integer(self.as_ptr(), digits.as_value().as_raw(), 16, false)
        });
        // SAFETY: the parse answers an Integer.
        unsafe { Integer::from_value_unchecked(v) }
    }
}

impl Fixnum {
    /// The same value as an `Integer`, which takes either tag.
    #[cfg(mrb_bigint)]
    fn as_integer(self) -> Integer {
        // SAFETY: a Fixnum is an Integer.
        unsafe { Integer::from_value_unchecked(self.as_value()) }
    }
}

#[cfg(mrb_bigint)]
impl RBignum {
    /// The same value as an `Integer`, which takes either tag.
    fn as_integer(self) -> Integer {
        // SAFETY: an RBignum is an Integer.
        unsafe { Integer::from_value_unchecked(self.as_value()) }
    }
}
