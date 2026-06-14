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

use crate::{Mrb, Value};
use beni_sys as sys;

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
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{Ccontext, Error, FromValue, IntoValue, Mrb, Range};

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
}
