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

use crate::{Error, Mrb, RString, Value};
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

    /// `mrb_ary_push(mrb, self, val)` — append `val`, the way Ruby's
    /// `Array#push` extends its receiver. Appending to a frozen array
    /// raises `FrozenError`; the call runs under `Mrb::protect`, so that
    /// surfaces as `Err` rather than long-jumping.
    #[inline]
    pub fn push(self, mrb: &Mrb, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Array-tagged by the `from_value_unchecked` contract;
                // `val` originates from the same VM. `mrb_ary_push` calls
                // `mrb_ary_modify`, which raises `FrozenError` on a frozen
                // array — caught by `protect` into `Err`.
                unsafe { sys::mrb_ary_push(mrb.as_ptr(), self.0.as_raw(), val.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
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

    /// `mrb_ary_set(mrb, self, idx, val)` — write `val` at `idx`,
    /// following Ruby's `ary[idx] = val`: a positive index past the end
    /// grows the array with `nil`, and a negative index counts from the
    /// tail. An index that reaches past the beginning, or one too large,
    /// raises `IndexError`, surfaced here as `Err`.
    #[inline]
    pub fn store(self, mrb: &Mrb, idx: isize, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            // An index outside the archive's `mrb_int` width names no
            // slot; saturate it to the nearest bound so mruby's own
            // range check raises the matching `IndexError` (too large,
            // or past the beginning) rather than a truncated index
            // silently hitting the wrong slot.
            let n = sys::mrb_int::try_from(idx).unwrap_or(if idx < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive; `self` is Array-tagged by the
                // `from_value_unchecked` contract; `val` shares the VM
                // by the single-VM contract. `mrb_ary_set` range-checks
                // `n` and may raise `IndexError`, which `protect` catches
                // into `Err`.
                unsafe { sys::mrb_ary_set(mrb.as_ptr(), self.0.as_raw(), n, val.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, idx, val);
            crate::not_linked()
        }
    }

    /// `mrb_ary_resize(mrb, self, new_len)` — set the array's length:
    /// grow with `nil` to reach a longer length, or truncate to a shorter
    /// one. Resizing a frozen array raises `FrozenError`; the call runs
    /// under `Mrb::protect`, so that surfaces as `Err`. `new_len`
    /// saturates to the archive's `mrb_int` width.
    #[inline]
    pub fn resize(self, mrb: &Mrb, new_len: usize) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            let new_len = new_len.min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Array-tagged by the `from_value_unchecked` contract.
                // `mrb_ary_resize` routes through `mrb_ary_modify`, which
                // raises `FrozenError` on a frozen array — caught by
                // `protect` into `Err`.
                unsafe { sys::mrb_ary_resize(mrb.as_ptr(), self.0.as_raw(), new_len) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, new_len);
            crate::not_linked()
        }
    }

    /// `mrb_ary_pop(mrb, self)` — remove and return the last element,
    /// Ruby's `Array#pop`, or `nil` when the array is empty. Popping a
    /// frozen array raises `FrozenError`, surfaced here as `Err`.
    #[inline]
    pub fn pop(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Array-tagged by the `from_value_unchecked` contract.
                // `mrb_ary_pop` checks frozen state and may raise
                // `FrozenError` — caught by `protect`.
                Value::from_raw(unsafe { sys::mrb_ary_pop(mrb.as_ptr(), self.0.as_raw()) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_ary_shift(mrb, self)` — remove and return the first element,
    /// Ruby's `Array#shift`, or `nil` when the array is empty. Shifting a
    /// frozen array raises `FrozenError`, surfaced as `Err`.
    #[inline]
    pub fn shift(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `pop`; `mrb_ary_shift` checks frozen state and
                // may raise `FrozenError` — caught by `protect`.
                Value::from_raw(unsafe { sys::mrb_ary_shift(mrb.as_ptr(), self.0.as_raw()) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_ary_unshift(mrb, self, val)` — prepend `val`, Ruby's
    /// `Array#unshift`. Prepending to a frozen array raises `FrozenError`,
    /// surfaced as `Err`.
    #[inline]
    pub fn unshift(self, mrb: &Mrb, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `push`; `mrb_ary_unshift` modifies and may
                // raise `FrozenError` — caught by `protect`.
                unsafe { sys::mrb_ary_unshift(mrb.as_ptr(), self.0.as_raw(), val.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, val);
            crate::not_linked()
        }
    }

    /// `mrb_ary_concat(mrb, self, other)` — append `other`'s elements,
    /// Ruby's `Array#concat`. Concatenating into a frozen array raises
    /// `FrozenError`, surfaced as `Err`.
    #[inline]
    pub fn concat(self, mrb: &Mrb, other: Array) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `push`; `self` and `other` are Array-tagged
                // and share the VM. `mrb_ary_concat` modifies `self` and
                // may raise `FrozenError` — caught by `protect`.
                unsafe { sys::mrb_ary_concat(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_ary_clear(mrb, self)` — remove all elements, Ruby's
    /// `Array#clear`. Clearing a frozen array raises `FrozenError`,
    /// surfaced as `Err`.
    #[inline]
    pub fn clear(self, mrb: &Mrb) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `push`; `mrb_ary_clear` modifies and may raise
                // `FrozenError` — caught by `protect`.
                unsafe { sys::mrb_ary_clear(mrb.as_ptr(), self.0.as_raw()) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_ary_join(mrb, self, sep)` — render the elements into one
    /// string with `sep` between them, Ruby's `Array#join`. Each element's
    /// `to_s` runs, so a raise inside it surfaces as `Err`; the call runs
    /// under `Mrb::protect`. A `None` separator joins with nothing between,
    /// the way Ruby's `join` treats a `nil` argument.
    #[inline]
    pub fn join(self, mrb: &Mrb, sep: Option<RString>) -> Result<RString, Error> {
        #[cfg(mruby_linked)]
        {
            let sep = sep.map_or_else(Value::nil, RString::as_value);
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Array-tagged by the `from_value_unchecked` contract;
                // `sep` is nil or a String-tagged value from the same VM.
                // `mrb_ary_join` dispatches each element's `to_s`, which may
                // raise — caught by `protect` into `Err`.
                Value::from_raw(unsafe {
                    sys::mrb_ary_join(mrb.as_ptr(), self.0.as_raw(), sep.as_raw())
                })
            })
            // SAFETY: `mrb_ary_join` returns a String-tagged value.
            .map(|v| unsafe { RString::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sep);
            crate::not_linked()
        }
    }

    /// `mrb_ary_dup(mrb, self)` — a shallow copy, Ruby's `Array#dup`. It
    /// does not mutate the receiver, so it never fails.
    #[inline]
    pub fn dup(self, mrb: &Mrb) -> Array {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is Array-tagged by the `from_value_unchecked`
            // contract; `mrb_ary_dup` returns a fresh Array-tagged value,
            // so the unchecked wrap is sound.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_ary_dup(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
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
    use crate::{Error, Mrb};

    #[test]
    fn push_and_entry_roundtrip_through_a_live_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        ary.push(&mrb, mrb.str_new(b"first").as_value())
            .expect("push to a fresh array succeeds");
        ary.push(&mrb, mrb.str_new(b"second").as_value())
            .expect("push to a fresh array succeeds");

        assert_eq!(ary.entry(0).to_string(&mrb), "first");
        assert_eq!(ary.entry(-1).to_string(&mrb), "second");
    }

    #[test]
    fn entry_is_nil_out_of_range_in_both_directions() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        ary.push(&mrb, mrb.str_new(b"only").as_value())
            .expect("push to a fresh array succeeds");

        assert!(ary.entry(1).is_nil());
        assert!(ary.entry(-2).is_nil());
        // An index beyond the archive's `mrb_int` width is out of
        // range by definition — same nil contract, no truncation.
        assert!(ary.entry(isize::MAX).is_nil());
        assert!(ary.entry(isize::MIN).is_nil());
    }

    #[test]
    fn store_writes_grows_and_counts_from_the_tail() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        // Writing past the end grows the array, filling the gap with nil.
        ary.store(&mrb, 2, mrb.str_new(b"two").as_value())
            .expect("an in-range store succeeds");
        assert_eq!(ary.len(), 3);
        assert!(ary.entry(0).is_nil());
        assert!(ary.entry(1).is_nil());
        assert_eq!(ary.entry(2).to_string(&mrb), "two");

        // A negative index counts from the tail.
        ary.store(&mrb, -1, mrb.str_new(b"last").as_value())
            .expect("a negative in-range store succeeds");
        assert_eq!(ary.entry(2).to_string(&mrb), "last");
    }

    #[test]
    fn store_out_of_range_index_surfaces_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, mrb.str_new(b"only").as_value())
            .expect("push to a fresh array succeeds");

        // A negative index reaching past the beginning raises IndexError.
        assert!(matches!(
            ary.store(&mrb, -5, mrb.str_new(b"x").as_value()),
            Err(Error::Exception(_))
        ));
        // An index beyond the archive's `mrb_int` width saturates so
        // mruby's own range check rejects it as too large, rather than a
        // truncated index hitting the wrong slot.
        assert!(matches!(
            ary.store(&mrb, isize::MAX, mrb.str_new(b"x").as_value()),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn len_and_is_empty_track_the_element_count() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        assert_eq!(ary.len(), 0);
        assert!(ary.is_empty());

        ary.push(&mrb, mrb.str_new(b"a").as_value())
            .expect("push to a fresh array succeeds");
        ary.push(&mrb, mrb.str_new(b"b").as_value())
            .expect("push to a fresh array succeeds");

        assert_eq!(ary.len(), 2);
        assert!(!ary.is_empty());
    }

    #[test]
    fn push_surfaces_frozen_receiver_as_err() {
        use crate::{Array, Ccontext, FromValue};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_ary.rb").expect("allocating the context must succeed");

        // A frozen Array still carries the Array tag, so the downcast
        // holds, but pushing to it raises FrozenError — which protect
        // catches into Err rather than long-jumping.
        let frozen = Array::from_value(cxt.load_nstring(b"[].freeze"))
            .expect("a frozen Array literal is Array-tagged");
        assert!(matches!(
            frozen.push(&mrb, mrb.str_new(b"x").as_value()),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn pop_and_shift_remove_from_each_end() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, mrb.str_new(b"a").as_value())
            .expect("push succeeds");
        ary.push(&mrb, mrb.str_new(b"b").as_value())
            .expect("push succeeds");

        assert_eq!(ary.pop(&mrb).expect("pop succeeds").to_string(&mrb), "b");
        assert_eq!(
            ary.shift(&mrb).expect("shift succeeds").to_string(&mrb),
            "a"
        );
        // Draining past empty yields nil, not an error.
        assert!(ary.pop(&mrb).expect("pop on empty succeeds").is_nil());
        assert!(ary.shift(&mrb).expect("shift on empty succeeds").is_nil());
    }

    #[test]
    fn unshift_prepends_and_concat_extends() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, mrb.str_new(b"mid").as_value())
            .expect("push succeeds");
        ary.unshift(&mrb, mrb.str_new(b"head").as_value())
            .expect("unshift succeeds");

        let tail = mrb.ary_new();
        tail.push(&mrb, mrb.str_new(b"tail").as_value())
            .expect("push succeeds");
        ary.concat(&mrb, tail).expect("concat succeeds");

        assert_eq!(ary.entry(0).to_string(&mrb), "head");
        assert_eq!(ary.entry(1).to_string(&mrb), "mid");
        assert_eq!(ary.entry(2).to_string(&mrb), "tail");
    }

    #[test]
    fn clear_empties_and_dup_copies_independently() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, mrb.str_new(b"x").as_value())
            .expect("push succeeds");

        let copy = ary.dup(&mrb);
        ary.clear(&mrb).expect("clear succeeds");

        // clear emptied the original; dup is an independent array.
        assert!(ary.is_empty());
        assert_eq!(copy.len(), 1);
        assert_eq!(copy.entry(0).to_string(&mrb), "x");
    }

    #[test]
    fn resize_grows_with_nil_and_truncates() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, mrb.str_new(b"a").as_value())
            .expect("push succeeds");

        // Growing past the current length fills the new slots with nil.
        ary.resize(&mrb, 3).expect("grow succeeds");
        assert_eq!(ary.len(), 3);
        assert_eq!(ary.entry(0).to_string(&mrb), "a");
        assert!(ary.entry(1).is_nil());
        assert!(ary.entry(2).is_nil());

        // Truncating drops the tail.
        ary.resize(&mrb, 1).expect("truncate succeeds");
        assert_eq!(ary.len(), 1);
        assert_eq!(ary.entry(0).to_string(&mrb), "a");
    }

    #[test]
    fn join_renders_elements_with_a_separator() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, crate::Value::from_int(&mrb, 1))
            .expect("push succeeds");
        ary.push(&mrb, crate::Value::from_int(&mrb, 2))
            .expect("push succeeds");
        ary.push(&mrb, crate::Value::from_int(&mrb, 3))
            .expect("push succeeds");

        // Each element's to_s runs and the separator sits between adjacent
        // renderings.
        let joined = ary
            .join(&mrb, Some(mrb.str_new(b",")))
            .expect("join with a separator succeeds");
        assert_eq!(joined.to_bytes(), b"1,2,3".to_vec());

        // A None separator concatenates the renderings with nothing between.
        let glued = ary
            .join(&mrb, None)
            .expect("join without a separator succeeds");
        assert_eq!(glued.to_bytes(), b"123".to_vec());
    }

    #[test]
    fn join_surfaces_a_raising_element_to_s_as_err() {
        use crate::{Array, Ccontext, FromValue};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"join_raise.rb").expect("allocating the context must succeed");

        // An element whose to_s raises long-jumps out of mrb_ary_join;
        // protect catches it into Err rather than unwinding across FFI.
        let ary = Array::from_value(
            cxt.load_nstring(b"o = Object.new; def o.to_s; raise 'boom'; end; [o]"),
        )
        .expect("an Array literal is Array-tagged");
        assert!(matches!(ary.join(&mrb, None), Err(Error::Exception(_))));
    }

    #[test]
    fn pop_surfaces_frozen_receiver_as_err() {
        use crate::{Array, Ccontext, FromValue};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_pop.rb").expect("allocating the context must succeed");

        // pop checks frozen state before touching the elements, so even a
        // populated frozen array surfaces FrozenError as Err.
        let frozen = Array::from_value(cxt.load_nstring(b"[1].freeze"))
            .expect("a frozen Array literal is Array-tagged");
        assert!(matches!(frozen.pop(&mrb), Err(Error::Exception(_))));
    }

    #[test]
    fn remaining_mutators_surface_frozen_receiver_as_err() {
        use crate::{Array, Ccontext, FromValue};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_ary_mut.rb").expect("allocating the context must succeed");

        // Every mutator routes through mrb_ary_modify, which raises
        // FrozenError on a frozen receiver — protect catches each into Err.
        // push and pop are pinned separately; this covers the rest.
        let frozen = Array::from_value(cxt.load_nstring(b"[1].freeze"))
            .expect("a frozen Array literal is Array-tagged");
        let other = mrb.ary_new();

        assert!(matches!(
            frozen.unshift(&mrb, mrb.str_new(b"x").as_value()),
            Err(Error::Exception(_))
        ));
        assert!(matches!(
            frozen.concat(&mrb, other),
            Err(Error::Exception(_))
        ));
        assert!(matches!(frozen.shift(&mrb), Err(Error::Exception(_))));
        assert!(matches!(frozen.clear(&mrb), Err(Error::Exception(_))));
        assert!(matches!(frozen.resize(&mrb, 5), Err(Error::Exception(_))));
        // An in-range indexed write reaches the frozen check too, not just
        // the out-of-range path pinned above.
        assert!(matches!(
            frozen.store(&mrb, 0, mrb.str_new(b"x").as_value()),
            Err(Error::Exception(_))
        ));
    }
}
