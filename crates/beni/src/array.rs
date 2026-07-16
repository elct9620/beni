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

    /// `mrb_ary_replace(mrb, self, other)` — make the receiver's contents
    /// a copy of `other`'s, in place, Ruby's `Array#replace`. Replacing a
    /// frozen receiver raises `FrozenError`, surfaced as `Err`.
    #[inline]
    pub fn replace(self, mrb: &Mrb, other: Array) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `concat`; `self` and `other` are Array-tagged
                // and share the VM. `mrb_ary_replace` modifies `self` and
                // may raise `FrozenError` — caught by `protect`.
                unsafe { sys::mrb_ary_replace(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) };
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

    /// `mrb_ary_splice(mrb, self, head, len, rpl)` — replace the `len`
    /// elements starting at `head` with `rpl`, in place, Ruby's
    /// `ary[head, len] = rpl`: the primitive behind indexed assignment,
    /// insertion, and deletion. An Array `rpl` splices in its elements;
    /// any other value is inserted as the single element it is — pass an
    /// empty array to delete without inserting. A `head` past the end
    /// grows the array with `nil` to reach it; a negative `head` counts
    /// from the tail. A frozen receiver, a negative `len`, or a `head`
    /// past the beginning raises, surfaced here as `Err`. Returns the
    /// receiver. `head` and `len` saturate to the archive's `mrb_int`
    /// width, so an out-of-width `head` keeps mruby's range check raising
    /// rather than a truncated index hitting the wrong slot.
    #[inline]
    pub fn splice(self, mrb: &Mrb, head: i64, len: i64, rpl: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            let head = sys::mrb_int::try_from(head).unwrap_or(if head < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            let len = sys::mrb_int::try_from(len).unwrap_or(if len < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is Array-tagged by the `from_value_unchecked` contract;
                // `rpl` shares the VM by the single-VM contract and is
                // handled for any tag (an array splices its elements, any
                // other value inserts as one — no unsafe unbox).
                // `mrb_ary_splice` routes through `mrb_ary_modify` and
                // range-checks `head`/`len`, raising `FrozenError` or
                // `IndexError` — caught by `protect` into `Err`.
                Value::from_raw(unsafe {
                    sys::mrb_ary_splice(mrb.as_ptr(), self.0.as_raw(), head, len, rpl.as_raw())
                })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, head, len, rpl);
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

    /// Walk the elements by C-level index — repeated `entry` reads over a
    /// length snapshot taken here, never a Ruby `#each` / `#[]` dispatch.
    /// Reach for it when a walk must not dispatch Ruby.
    ///
    /// This is a live view, not a content snapshot: a re-entrant mutation
    /// during the walk is only partly visible — an element appended past the
    /// snapshot length is not visited, a position the array no longer reaches
    /// reads `nil`, and a position whose element changed reads its current
    /// value. Capture the elements as they stand by duplicating the array
    /// first.
    #[inline]
    pub fn entries(self) -> Entries {
        Entries {
            ary: self,
            idx: 0,
            len: self.len(),
        }
    }
}

/// Iterator returned by `Array::entries`. Reads each slot through `entry`
/// against the length fixed when the walk began, yielding `nil` for any
/// position the array no longer reaches. `ExactSizeIterator` reports that
/// fixed length: exactly as many items as the array held at the walk's
/// start, regardless of a mutation during it.
pub struct Entries {
    ary: Array,
    idx: usize,
    len: usize,
}

impl Iterator for Entries {
    type Item = Value;

    #[inline]
    fn next(&mut self) -> Option<Value> {
        if self.idx >= self.len {
            return None;
        }
        let v = self.ary.entry(self.idx as isize);
        self.idx += 1;
        Some(v)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.idx;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Entries {}

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
    fn replace_swaps_the_whole_contents_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, crate::Value::from_int(&mrb, 1))
            .expect("push succeeds");
        ary.push(&mrb, crate::Value::from_int(&mrb, 2))
            .expect("push succeeds");

        let other = mrb.ary_new();
        for n in [3, 4, 5] {
            other
                .push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }

        ary.replace(&mrb, other).expect("replace succeeds");

        // The receiver now holds a copy of other's elements, in place.
        assert_eq!(ary.len(), 3);
        assert_eq!(ary.entry(0).to_string(&mrb), "3");
        assert_eq!(ary.entry(1).to_string(&mrb), "4");
        assert_eq!(ary.entry(2).to_string(&mrb), "5");
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
    fn splice_inserts_replaces_and_deletes_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        for n in [1, 2, 3] {
            ary.push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }

        // A zero-length splice inserts the replacement's elements without
        // removing any: [1,2,3] -> [1,10,11,2,3].
        let ins = mrb.ary_new();
        for n in [10, 11] {
            ins.push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }
        ary.splice(&mrb, 1, 0, ins.as_value())
            .expect("a zero-length splice succeeds");
        assert_eq!(ary.len(), 5);
        assert_eq!(ary.entry(1).to_string(&mrb), "10");
        assert_eq!(ary.entry(2).to_string(&mrb), "11");

        // A non-array replacement is inserted as the single element it is,
        // replacing the run in place: [1,10,11,2,3] -> [1,10,99,2,3].
        ary.splice(&mrb, 2, 1, crate::Value::from_int(&mrb, 99))
            .expect("an in-place single-element replace succeeds");
        assert_eq!(ary.len(), 5);
        assert_eq!(ary.entry(2).to_string(&mrb), "99");

        // Replacing with fewer elements than removed shrinks the array;
        // an empty replacement deletes outright: removing the two slots at
        // index 2 leaves [1,10,3].
        ary.splice(&mrb, 2, 2, mrb.ary_new().as_value())
            .expect("a shrinking delete-and-replace succeeds");
        assert_eq!(ary.len(), 3);
        assert_eq!(ary.entry(0).to_string(&mrb), "1");
        assert_eq!(ary.entry(1).to_string(&mrb), "10");
        assert_eq!(ary.entry(2).to_string(&mrb), "3");

        // The return value is the receiver itself.
        let returned = ary
            .splice(&mrb, 0, 0, mrb.ary_new().as_value())
            .expect("a no-op splice succeeds");
        assert_eq!(returned.to_string(&mrb), ary.as_value().to_string(&mrb));
    }

    #[test]
    fn splice_surfaces_raising_edges_as_err() {
        use crate::{Array, Ccontext, FromValue};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, crate::Value::from_int(&mrb, 1))
            .expect("push succeeds");

        // A head reaching past the beginning raises IndexError.
        assert!(matches!(
            ary.splice(&mrb, -5, 0, mrb.ary_new().as_value()),
            Err(Error::Exception(_))
        ));
        // A negative length raises IndexError.
        assert!(matches!(
            ary.splice(&mrb, 0, -1, mrb.ary_new().as_value()),
            Err(Error::Exception(_))
        ));
        // A head beyond the archive's mrb_int width saturates so mruby's
        // own range check rejects it as out of array, rather than a
        // truncated index hitting the wrong slot.
        assert!(matches!(
            ary.splice(&mrb, i64::MAX, 0, mrb.ary_new().as_value()),
            Err(Error::Exception(_))
        ));

        // A frozen receiver raises FrozenError before any work — splice
        // routes through mrb_ary_modify like the other mutators.
        let cxt =
            Ccontext::new(&mrb, c"frozen_splice.rb").expect("allocating the context must succeed");
        let frozen = Array::from_value(cxt.load_nstring(b"[1].freeze"))
            .expect("a frozen Array literal is Array-tagged");
        assert!(matches!(
            frozen.splice(&mrb, 0, 1, mrb.ary_new().as_value()),
            Err(Error::Exception(_))
        ));
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
        assert!(matches!(
            frozen.replace(&mrb, other),
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

    #[test]
    fn entries_visits_nothing_for_an_empty_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();

        // A length-0 walk yields no elements at all.
        assert_eq!(ary.entries().count(), 0);
    }

    #[test]
    fn entries_walks_elements_first_to_last() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        for n in [1, 2, 3] {
            ary.push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }

        // The count is exact up front (ExactSizeIterator), and the walk reads
        // the slots from the first to the last in order.
        assert_eq!(ary.entries().len(), 3);
        let rendered: Vec<String> = ary.entries().map(|v| v.to_string(&mrb)).collect();
        assert_eq!(rendered, ["1", "2", "3"]);
    }

    #[test]
    fn entries_snapshots_the_length_so_a_shrink_reads_nil_past_the_new_end() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        for n in [1, 2, 3] {
            ary.push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }

        // The walk fixes its length at 3 when it begins. Shrinking the array
        // to one element mid-walk does not shorten the walk: the first slot
        // reads its live value, and the two positions the array no longer
        // reaches read nil. Re-reading the length each step would instead have
        // stopped after the single live element.
        let mut walk = ary.entries();
        assert_eq!(
            walk.next()
                .expect("the first slot is visited")
                .to_string(&mrb),
            "1"
        );

        ary.pop(&mrb).expect("pop succeeds");
        ary.pop(&mrb).expect("pop succeeds");
        assert_eq!(ary.len(), 1);

        assert!(walk
            .next()
            .expect("the second slot is still visited")
            .is_nil());
        assert!(walk
            .next()
            .expect("the third slot is still visited")
            .is_nil());
        assert!(walk.next().is_none());
    }

    #[test]
    fn entries_does_not_visit_elements_appended_after_the_walk_begins() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let ary = mrb.ary_new();
        ary.push(&mrb, crate::Value::from_int(&mrb, 1))
            .expect("push succeeds");

        // The walk fixes its length at 1 when it begins. Growing the array
        // mid-walk does not lengthen the walk: it ends after the one element
        // present at the start, never reaching the appended tail.
        let mut walk = ary.entries();
        assert_eq!(
            walk.next()
                .expect("the first slot is visited")
                .to_string(&mrb),
            "1"
        );

        for n in [2, 3] {
            ary.push(&mrb, crate::Value::from_int(&mrb, n))
                .expect("push succeeds");
        }
        assert_eq!(ary.len(), 3);

        assert!(walk.next().is_none());
    }
}
