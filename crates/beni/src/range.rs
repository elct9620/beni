//! Typed `Range` newtype around a Range-tagged `Value`.
//!
//! `Range` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Range` is exactly an `mrb_value` known to carry
//! an mruby `Range`. Construction is by `Mrb::range_new` or an explicit
//! unchecked cast from `Value`; the bound reads cluster on the newtype.
//!
//! Mirrors magnus's `src/r_range.rs`: the `range_new` factory lives on
//! `Mrb`, the begin / end / exclusive-end reads live here.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

/// The three-way outcome of `Range::beg_len` — the normalized slice a
/// `Range` covers of a collection of a given length, mruby's
/// `mrb_range_beg_len`. `Out` and `TypeMismatch` are kept distinct (not
/// collapsed into one absence) so a caller can tell an out-of-range
/// Range from a non-Range receiver.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RangeBegLen {
    /// The Range maps onto the collection: `beg` is the normalized
    /// begin offset (negative bounds counted back from the length) and
    /// `len` is the selected length.
    Ok {
        /// Normalized begin offset into the collection.
        beg: sys::mrb_int,
        /// Selected length from `beg`.
        len: sys::mrb_int,
    },
    /// The begin offset falls outside the collection (before its start,
    /// or — when truncating — past its end). Carries no offsets.
    Out,
    /// The receiver is not a Range. Carries no offsets.
    TypeMismatch,
}

/// Typed handle on an mruby `Range`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::range_new` (fresh range), the checked
/// `FromValue` downcast (`Range::from_value`, tag-discriminated), or
/// `Range::from_value_unchecked` (assert that a `Value` you already
/// hold is Range-tagged). Round-trip back to a generic `Value` via
/// `Range::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Range(Value);

impl Range {
    /// Wrap a `Value` that the caller has already determined to be
    /// Range-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_range_new`).
    ///
    /// # Safety
    ///
    /// `v` must be Range-tagged. Operating on a non-Range value
    /// through this newtype is undefined per mruby's macro contract
    /// (the underlying `mrb_range_*` reads assume `RRange` layout).
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not yet
    /// migrated. Same conversion ladder as `Value::as_raw`.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_range_beg(mrb, self)` — the begin value, Ruby's
    /// `Range#begin`, via the `mrb_range_beg_func` shim (the macro
    /// expanded in the C compiler so the embed-vs-edges `RRange` read
    /// matches the linked archive). A pure field read that dispatches
    /// nothing, so it never raises.
    #[inline]
    pub fn begin(self, mrb: &Mrb) -> Value {
        // SAFETY: `self` is Range-tagged by the `from_value_unchecked`
        // contract; `mrb` is alive by the borrow. `mrb_range_beg_func`
        // reads only the `RRange` begin field.
        Value::from_raw(unsafe { sys::mrb_range_beg_func(mrb.as_ptr(), self.0.as_raw()) })
    }

    /// `mrb_range_end(mrb, self)` — the end value, Ruby's `Range#end`,
    /// the mirror of `begin`. Named `end_` because `end` is a Rust
    /// keyword; a pure field read that never raises.
    #[inline]
    pub fn end_(self, mrb: &Mrb) -> Value {
        // SAFETY: as `begin`; `mrb_range_end_func` reads only the
        // `RRange` end field.
        Value::from_raw(unsafe { sys::mrb_range_end_func(mrb.as_ptr(), self.0.as_raw()) })
    }

    /// `mrb_range_excl_p(mrb, self)` — TRUE when the range excludes its
    /// end value, Ruby's `Range#exclude_end?`. A pure flag read that
    /// dispatches nothing, so it never raises.
    #[inline]
    pub fn is_exclusive(self, mrb: &Mrb) -> bool {
        // SAFETY: as `begin`; `mrb_range_excl_p_func` reads only the
        // `RRange` exclude-end flag.
        unsafe { sys::mrb_range_excl_p_func(mrb.as_ptr(), self.0.as_raw()) }
    }

    /// `mrb_range_beg_len(mrb, self, &beg, &len, len, trunc)` — the
    /// normalized slice this Range covers of a collection `len` long, the
    /// primitive behind slicing a collection by a Range (Ruby's
    /// `Array#[range]` / `String#[range]`). Negative or missing bounds are
    /// resolved against `len`; `trunc` treats a begin past the length as
    /// out-of-range and clamps an over-long end to the length.
    ///
    /// Returns the three-way `RangeBegLen` outcome — in-range, out-of-range,
    /// or non-Range mismatch — kept distinct so the caller can tell them
    /// apart. It dispatches nothing, but coercing a non-integer bound raises
    /// `TypeError`; the call runs under `Mrb::protect`, so that surfaces as
    /// `Err` rather than long-jumping. Mirrors magnus's `Range::beg_len`,
    /// which collapses the two non-`Ok` outcomes into one `Err`.
    #[inline]
    pub fn beg_len(self, mrb: &Mrb, len: i64, trunc: bool) -> Result<RangeBegLen, Error> {
        use core::cell::Cell;

        // The outcome and out-params live on this frame so the protected
        // closure only borrows them (all `Copy`); the raise long-jump,
        // which does not run Rust drops, leaves them owned here.
        let outcome: Cell<sys::mrb_range_beg_len> = Cell::new(sys::MRB_RANGE_TYPE_MISMATCH);
        let begp: Cell<sys::mrb_int> = Cell::new(0);
        let lenp: Cell<sys::mrb_int> = Cell::new(0);

        // A length wider than the archive's `mrb_int` names no
        // reachable extent; saturate it up (length is non-negative) so
        // the clamp sees "as large as representable" rather than a
        // wrapped value landing on a wrong span.
        let len = sys::mrb_int::try_from(len).unwrap_or(sys::mrb_int::MAX);

        mrb.protect(|mrb| {
            let mut beg: sys::mrb_int = 0;
            let mut sel: sys::mrb_int = 0;
            // SAFETY: `self` is Range-tagged by the newtype contract (a
            // non-Range receiver returns `MRB_RANGE_TYPE_MISMATCH` without
            // a field read); `mrb` is alive inside the protect frame.
            // `mrb_range_beg_len` writes `beg`/`sel` only on
            // `MRB_RANGE_OK`. A non-integer bound raises `TypeError`,
            // caught by `protect` into `Err`.
            outcome.set(unsafe {
                sys::mrb_range_beg_len(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    &mut beg,
                    &mut sel,
                    len,
                    trunc,
                )
            });
            begp.set(beg);
            lenp.set(sel);
            Value::nil()
        })?;

        Ok(match outcome.get() {
            sys::MRB_RANGE_OK => RangeBegLen::Ok {
                beg: begp.get(),
                len: lenp.get(),
            },
            sys::MRB_RANGE_OUT => RangeBegLen::Out,
            _ => RangeBegLen::TypeMismatch,
        })
    }
}
