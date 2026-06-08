//! Typed `Array` newtype around an Array-tagged `Value`.
//!
//! `Array` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Array` is exactly an `mrb_value` known to carry
//! an mruby `Array`. Construction is by explicit unchecked cast from
//! `Value`; element operations cluster on the resulting newtype.
//!
//! Mirrors magnus's `src/r_array.rs`: container factories live on
//! `Ruby` / `Mrb` (`ary_new`, `hash_new`), per-array ops (`push`,
//! `entry`) live here. Named-value constructors that magnus places on
//! the type itself stay there too (`Symbol::new`).

use crate::{Mrb, Value};
use beni_sys as sys;

/// Typed handle on an mruby `Array`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::ary_new` (fresh array), the checked
/// `FromValue` downcast (`Array::from_value`, tag-discriminated), or
/// `Array::from_value_unchecked` (assert that a `Value` you
/// already hold is Array-tagged). Round-trip back to a generic
/// `Value` via `Array::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Array(Value);

impl Array {
    /// Wrap a `Value` that the caller has already determined to be
    /// Array-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_ary_new` / a host array decoder).
    ///
    /// # Safety
    ///
    /// `v` must be Array-tagged. Operating on a non-Array value
    /// through this newtype is undefined per mruby's macro contract
    /// (the underlying `mrb_ary_*` calls assume Array layout).
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not
    /// yet migrated. Same conversion ladder as
    /// `Value::as_raw`.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_ary_push(mrb, self, val)` — append `val` to this array.
    #[inline]
    pub fn push(self, mrb: &Mrb, val: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` is Array-tagged by the
            // `from_value_unchecked` contract; `val` originates from the
            // same VM by the single-VM contract.
            unsafe { sys::mrb_ary_push(mrb.as_ptr(), self.0.as_raw(), val.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, val);
            crate::not_linked()
        }
    }

    /// `mrb_ary_entry(self, idx)` — read the element at `idx`
    /// (negative counts from the tail). Returns `mrb_nil_value()`
    /// when `idx` is out of range — mruby's own bounds-tolerant
    /// behaviour; an `idx` beyond the archive's `mrb_int` width
    /// cannot address an element, so it is out of range by
    /// definition. The type guarantee from the constructor makes
    /// this safe for any `idx`.
    #[inline]
    pub fn entry(self, idx: isize) -> Value {
        #[cfg(mruby_linked)]
        {
            let Ok(idx) = sys::mrb_int::try_from(idx) else {
                return Value::nil();
            };
            // SAFETY: `self` is Array-tagged by the `from_value_unchecked`
            // contract; `mrb_ary_entry` is bounds-tolerant.
            Value::from_raw(unsafe { sys::mrb_ary_entry(self.0.as_raw(), idx) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = idx;
            crate::not_linked()
        }
    }

    /// `RARRAY_LEN(self)` — the number of elements, via the
    /// `mrb_rarray_len_func` shim (the macro expanded in the C compiler
    /// so the embed-vs-heap length read matches the linked archive's
    /// layout). An mruby array length is never negative, so the result
    /// is returned as `usize`.
    #[inline]
    pub fn len(self) -> usize {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Array-tagged by the `from_value_unchecked`
            // contract; `RARRAY_LEN` reads only the array header.
            (unsafe { sys::mrb_rarray_len_func(self.0.as_raw()) }) as usize
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when the array holds no elements.
    #[inline]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::Mrb;

    #[test]
    fn push_and_entry_roundtrip_through_a_live_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        ary.push(&mrb, mrb.str_new(b"first"));
        ary.push(&mrb, mrb.str_new(b"second"));

        assert_eq!(ary.entry(0).to_string(&mrb), "first");
        assert_eq!(ary.entry(-1).to_string(&mrb), "second");
    }

    #[test]
    fn entry_is_nil_out_of_range_in_both_directions() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        ary.push(&mrb, mrb.str_new(b"only"));

        assert!(ary.entry(1).is_nil());
        assert!(ary.entry(-2).is_nil());
        // An index beyond the archive's `mrb_int` width is out of
        // range by definition — same nil contract, no truncation.
        assert!(ary.entry(isize::MAX).is_nil());
        assert!(ary.entry(isize::MIN).is_nil());
    }

    #[test]
    fn len_and_is_empty_track_the_element_count() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        assert_eq!(ary.len(), 0);
        assert!(ary.is_empty());

        ary.push(&mrb, mrb.str_new(b"a"));
        ary.push(&mrb, mrb.str_new(b"b"));

        assert_eq!(ary.len(), 2);
        assert!(!ary.is_empty());
    }
}
