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
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Range-tagged by the `from_value_unchecked`
            // contract; `mrb` is alive by the borrow. `mrb_range_beg_func`
            // reads only the `RRange` begin field.
            Value::from_raw(unsafe { sys::mrb_range_beg_func(mrb.as_ptr(), self.0.as_raw()) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_range_end(mrb, self)` — the end value, Ruby's `Range#end`,
    /// the mirror of `begin`. Named `end_` because `end` is a Rust
    /// keyword; a pure field read that never raises.
    #[inline]
    pub fn end_(self, mrb: &Mrb) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `begin`; `mrb_range_end_func` reads only the
            // `RRange` end field.
            Value::from_raw(unsafe { sys::mrb_range_end_func(mrb.as_ptr(), self.0.as_raw()) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_range_excl_p(mrb, self)` — TRUE when the range excludes its
    /// end value, Ruby's `Range#exclude_end?`. A pure flag read that
    /// dispatches nothing, so it never raises.
    #[inline]
    pub fn is_exclusive(self, mrb: &Mrb) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `begin`; `mrb_range_excl_p_func` reads only the
            // `RRange` exclude-end flag.
            unsafe { sys::mrb_range_excl_p_func(mrb.as_ptr(), self.0.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, len, trunc);
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{sys, Ccontext, Error, FromValue, IntoValue, Mrb, Range, RangeBegLen};

    #[test]
    fn range_new_constructs_and_reads_back_its_bounds() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An inclusive integer range round-trips its begin, end, and
        // exclude-end flag.
        let r = mrb
            .range_new(0.into_value(&mrb), 10.into_value(&mrb), false)
            .expect("a comparable integer range constructs");
        assert_eq!(i32::from_value(r.begin(&mrb)), Some(0));
        assert_eq!(i32::from_value(r.end_(&mrb)), Some(10));
        assert!(!r.is_exclusive(&mrb));
    }

    #[test]
    fn range_new_carries_the_exclusive_flag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let r = mrb
            .range_new(1.into_value(&mrb), 5.into_value(&mrb), true)
            .expect("a comparable integer range constructs");
        assert!(r.is_exclusive(&mrb));
    }

    #[test]
    fn range_new_surfaces_incomparable_bounds_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A String and an Integer share no ordering, so mruby raises
        // ArgumentError ("bad value for range") — protect catches it into
        // Err rather than long-jumping across FFI.
        let result = mrb.range_new(mrb.str_new(b"a").as_value(), 42.into_value(&mrb), false);
        assert!(matches!(result, Err(Error::Exception(_))));
    }

    #[test]
    fn from_value_downcasts_by_the_range_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context must succeed");

        // A Range-tagged value downcasts to the typed handle; a non-range
        // tag rejects instead of wrapping a value the range reads would
        // misread.
        let range_val = cxt.load_nstring(b"(1..5)");
        assert!(
            mrb.pending_exc().is_nil(),
            "building the range literal must not raise"
        );
        assert!(Range::from_value(range_val).is_some());
        assert!(Range::from_value(42.into_value(&mrb)).is_none());
    }

    #[test]
    fn reads_track_an_exclusive_literal() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context must succeed");

        // A `(1...5)` literal is exclusive; its bounds read back unchanged.
        let r = Range::from_value(cxt.load_nstring(b"(1...5)"))
            .expect("a Range literal is Range-tagged");
        assert_eq!(i32::from_value(r.begin(&mrb)), Some(1));
        assert_eq!(i32::from_value(r.end_(&mrb)), Some(5));
        assert!(r.is_exclusive(&mrb));
    }

    #[test]
    fn beg_len_maps_an_in_range_slice() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

        // `2..7` against a length-10 collection selects 6 elements from
        // offset 2 (inclusive end, so 7 - 2 + 1). A negative end counts
        // back from the length.
        let r = Range::from_value(cxt.load_nstring(b"(2..7)")).expect("a Range literal");
        assert_eq!(
            r.beg_len(&mrb, 10, false)
                .expect("an integer range never raises"),
            RangeBegLen::Ok { beg: 2, len: 6 }
        );

        let r = Range::from_value(cxt.load_nstring(b"(-3..-1)")).expect("a Range literal");
        assert_eq!(
            r.beg_len(&mrb, 10, false)
                .expect("an integer range never raises"),
            RangeBegLen::Ok { beg: 7, len: 3 }
        );
    }

    #[test]
    fn beg_len_reports_a_begin_before_the_start_as_out() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

        // `-20` counts back past the start of a length-10 collection, so
        // the begin offset is out of range.
        let r = Range::from_value(cxt.load_nstring(b"(-20..-1)")).expect("a Range literal");
        assert_eq!(
            r.beg_len(&mrb, 10, false)
                .expect("an integer range never raises"),
            RangeBegLen::Out
        );
    }

    #[test]
    fn beg_len_truncates_an_over_long_end() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

        // `2..100` overruns a length-10 collection. With truncation the end
        // clamps to the length, selecting offsets 2 through 9; without it
        // the raw bounds yield a longer span.
        let r = Range::from_value(cxt.load_nstring(b"(2..100)")).expect("a Range literal");
        assert_eq!(
            r.beg_len(&mrb, 10, true)
                .expect("an integer range never raises"),
            RangeBegLen::Ok { beg: 2, len: 8 }
        );
        assert_eq!(
            r.beg_len(&mrb, 10, false)
                .expect("an integer range never raises"),
            RangeBegLen::Ok { beg: 2, len: 99 }
        );
    }

    #[test]
    fn beg_len_saturates_a_length_past_the_mrb_int_width() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

        // A length wider than `mrb_int` saturates up to the widest
        // representable extent, so truncating `2..7` still selects offsets
        // 2 through 7. A wrapping cast would land on a negative length, and
        // truncation against it would report the begin as out of range.
        let huge = i64::from(sys::mrb_int::MAX) + 1;
        let r = Range::from_value(cxt.load_nstring(b"(2..7)")).expect("a Range literal");
        assert_eq!(
            r.beg_len(&mrb, huge, true)
                .expect("an integer range never raises"),
            RangeBegLen::Ok { beg: 2, len: 6 }
        );
    }

    #[test]
    fn beg_len_rejects_a_non_range_as_mismatch() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A non-Range receiver wrapped through the unchecked cast reports a
        // type mismatch rather than reading a malformed field.
        let not_a_range = unsafe { Range::from_value_unchecked(42.into_value(&mrb)) };
        assert_eq!(
            not_a_range
                .beg_len(&mrb, 10, false)
                .expect("a mismatch is a return, not a raise"),
            RangeBegLen::TypeMismatch
        );
    }
}
