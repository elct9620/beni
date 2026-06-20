//! String / Array / Hash / Range factories on `Mrb`.
//!
//! `str_new` / `str_new_cstr` construct mruby Strings from Rust byte
//! slices or a NUL-terminated `&CStr`; `str_new_static` aliases a
//! `'static` buffer without copying. `ary_new` / `hash_new` return
//! typed `Array` / `Hash` newtypes, and `range_new` returns a typed
//! `Range` — per-collection operations (`push`, `set`, `get`, `keys`,
//! the range bound reads) live on the value newtype rather than on
//! `Mrb` so the call shape mirrors Ruby (`arr.push(x)`, not
//! `mrb.ary_push(arr, x)`). `range_new` is the one factory that can
//! raise — comparing incomparable bounds — so it returns a `Result`.

use crate::{Array, Error, Hash, Mrb, RString, Range, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

impl Mrb {
    /// `mrb_str_new(mrb, p, len)` — construct an mruby `String` from
    /// `bytes`. The buffer is copied into the mruby heap; the slice
    /// only has to live for the duration of the call.
    ///
    /// `bytes.len()` saturates to `sys::mrb_int::MAX` (the archive's
    /// configured integer width). Real callers stay far below that.
    #[inline]
    pub fn str_new(&self, bytes: &[u8]) -> RString {
        #[cfg(mruby_linked)]
        {
            let len = bytes.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive by the `&self` borrow; `bytes`
            // outlives the synchronous call. `mrb_str_new` always returns
            // a String-tagged value, so the unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_new(
                    self.as_ptr(),
                    bytes.as_ptr() as *const core::ffi::c_char,
                    len,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = bytes;
            crate::not_linked()
        }
    }

    /// `mrb_str_new_cstr(mrb, s)` — construct an mruby `String` from
    /// a NUL-terminated C string. The `&CStr` borrow guarantees the
    /// terminator.
    #[inline]
    pub fn str_new_cstr(&self, s: &core::ffi::CStr) -> RString {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
            // the `&CStr` contract. `mrb_str_new_cstr` always returns a
            // String-tagged value, so the unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_new_cstr(
                    self.as_ptr(),
                    s.as_ptr(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = s;
            crate::not_linked()
        }
    }

    /// `mrb_str_new_capa(mrb, capa)` — construct an empty mruby `String`
    /// with `capa` bytes of buffer reserved up front, sparing the
    /// reallocs a run of `cat` onto a fresh string would otherwise
    /// trigger (Ruby's `String.new(capacity:)`). The string starts empty;
    /// `capa` is a hint, not content. `capa` saturates to the archive's
    /// `mrb_int` width, mirroring `ary_new_capa`.
    #[inline]
    pub fn str_new_capa(&self, capa: usize) -> RString {
        #[cfg(mruby_linked)]
        {
            let capa = capa.min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive; `mrb_str_new_capa` always returns
            // a String-tagged value, so the unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_new_capa(
                    self.as_ptr(),
                    capa,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = capa;
            crate::not_linked()
        }
    }

    /// `mrb_str_new_static(mrb, p, len)` — construct an mruby `String`
    /// that aliases `bytes` without copying them, the no-copy counterpart
    /// of `str_new`. The `'static` bound is what makes this safe: mruby
    /// never frees the borrowed buffer, so it must outlive the VM, which a
    /// `'static` slice guarantees. The string is copy-on-write — an
    /// in-place append or resize reallocates first, leaving the borrowed
    /// bytes untouched. A `b"..."` literal is a `&'static [u8]`, so this
    /// also serves mruby's `mrb_str_new_lit` convenience.
    ///
    /// `bytes.len()` saturates to `sys::mrb_int::MAX` (the archive's
    /// configured integer width). Real callers stay far below that.
    #[inline]
    pub fn str_new_static(&self, bytes: &'static [u8]) -> RString {
        #[cfg(mruby_linked)]
        {
            let len = bytes.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive by the `&self` borrow; `bytes` is
            // `'static`, so the aliased buffer outlives the VM as mruby's
            // NOFREE contract requires. `mrb_str_new_static` always returns
            // a String-tagged value, so the unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_new_static(
                    self.as_ptr(),
                    bytes.as_ptr() as *const core::ffi::c_char,
                    len,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = bytes;
            crate::not_linked()
        }
    }

    /// `mrb_ary_new(mrb)` — construct a fresh empty mruby `Array` as
    /// a typed `Array`. Element operations (`push`, `entry`) live
    /// on the returned newtype.
    #[inline]
    pub fn ary_new(&self) -> Array {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `mrb_ary_new` always returns an
            // Array-tagged value, so the unchecked wrap is sound.
            unsafe { Array::from_value_unchecked(Value::from_raw(sys::mrb_ary_new(self.as_ptr()))) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_ary_new_capa(mrb, capa)` — construct an empty mruby `Array`
    /// with room preallocated for `capa` elements, sparing the reallocs
    /// a run of `push` onto a fresh array would otherwise trigger.
    /// `capa` saturates to the archive's `mrb_int` width.
    #[inline]
    pub fn ary_new_capa(&self, capa: usize) -> Array {
        #[cfg(mruby_linked)]
        {
            let capa = capa.min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive; `mrb_ary_new_capa` always returns
            // an Array-tagged value, so the unchecked wrap is sound.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_ary_new_capa(
                    self.as_ptr(),
                    capa,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = capa;
            crate::not_linked()
        }
    }

    /// `mrb_ary_new_from_values(mrb, len, vals)` — construct an mruby
    /// `Array` holding a copy of `values`, in order. `values.len()`
    /// saturates to the archive's `mrb_int` width.
    #[inline]
    pub fn ary_new_from_values(&self, values: &[Value]) -> Array {
        #[cfg(mruby_linked)]
        {
            let len = values.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive; `Value` is `#[repr(transparent)]`
            // over `mrb_value` (pinned by the ABI test), so the slice
            // pointer is a valid `*const mrb_value` for `len` elements,
            // which the call copies before returning.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_ary_new_from_values(
                    self.as_ptr(),
                    len,
                    values.as_ptr() as *const sys::mrb_value,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = values;
            crate::not_linked()
        }
    }

    /// `mrb_hash_new(mrb)` — construct a fresh empty mruby `Hash` as
    /// a typed `Hash`. Element operations (`set`, `get`, `keys`)
    /// live on the returned newtype.
    #[inline]
    pub fn hash_new(&self) -> Hash {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `mrb_hash_new` always returns a
            // Hash-tagged value, so the unchecked wrap is sound.
            unsafe { Hash::from_value_unchecked(Value::from_raw(sys::mrb_hash_new(self.as_ptr()))) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_hash_new_capa(mrb, capa)` — construct an empty mruby `Hash`
    /// with room preallocated for `capa` entries, sparing the reallocs
    /// a run of `set` onto a fresh hash would otherwise trigger. `capa`
    /// saturates to the archive's `mrb_int` width.
    #[inline]
    pub fn hash_new_capa(&self, capa: usize) -> Hash {
        #[cfg(mruby_linked)]
        {
            let capa = capa.min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive; `mrb_hash_new_capa` always returns
            // a Hash-tagged value, so the unchecked wrap is sound.
            unsafe {
                Hash::from_value_unchecked(Value::from_raw(sys::mrb_hash_new_capa(
                    self.as_ptr(),
                    capa,
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = capa;
            crate::not_linked()
        }
    }

    /// `mrb_assoc_new(mrb, car, cdr)` — construct the two-element mruby
    /// `Array` `[car, cdr]`. A pure allocation that copies the two values
    /// into a fresh array, so it never raises.
    #[inline]
    pub fn assoc_new(&self, car: Value, cdr: Value) -> Array {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `car` and `cdr` share the VM by the
            // single-VM contract. `mrb_assoc_new` always returns an
            // Array-tagged value, so the unchecked wrap is sound.
            unsafe {
                Array::from_value_unchecked(Value::from_raw(sys::mrb_assoc_new(
                    self.as_ptr(),
                    car.as_raw(),
                    cdr.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (car, cdr);
            crate::not_linked()
        }
    }

    /// `mrb_range_new(mrb, begin, end, exclusive)` — construct an mruby
    /// `Range` from a begin value, an end value, and an exclusive-end
    /// flag, Ruby's `Range.new(begin, end, exclusive)`. mruby compares
    /// the two bounds and raises `ArgumentError` ("bad value for range")
    /// when they cannot be compared; the call runs under `Mrb::protect`,
    /// so that surfaces as `Err` rather than long-jumping. `nil` bounds
    /// and a numeric pair always succeed.
    #[inline]
    pub fn range_new(&self, begin: Value, end: Value, exclusive: bool) -> Result<Range, Error> {
        #[cfg(mruby_linked)]
        {
            self.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `begin`
                // and `end` share the VM by the single-VM contract.
                // `mrb_range_new` compares the bounds and raises
                // `ArgumentError` on an incomparable pair — caught by
                // `protect` into `Err`.
                Value::from_raw(unsafe {
                    sys::mrb_range_new(mrb.as_ptr(), begin.as_raw(), end.as_raw(), exclusive)
                })
            })
            // SAFETY: an `Ok` result came from `mrb_range_new`, which
            // returns a Range-tagged value on success.
            .map(|v| unsafe { Range::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (begin, end, exclusive);
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::Mrb;

    #[test]
    fn str_factories_roundtrip_their_bytes() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        assert_eq!(
            mrb.str_new(b"from bytes").as_value().to_string(&mrb),
            "from bytes"
        );
        assert_eq!(
            mrb.str_new_cstr(c"from cstr").as_value().to_string(&mrb),
            "from cstr"
        );
    }

    #[test]
    fn str_new_capa_preallocates_an_empty_string() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Capacity is a hint, not content — the string starts empty and
        // fills as usual through cat.
        let s = mrb.str_new_capa(16);
        assert!(s.is_empty());
        s.cat(&mrb, b"reserved")
            .expect("append to a fresh string succeeds");
        assert_eq!(s.to_bytes(), b"reserved".to_vec());
    }

    #[test]
    fn str_new_static_aliases_a_static_buffer_without_copying() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A byte-string literal is a `&'static [u8]`, the same path mruby's
        // `mrb_str_new_lit` macro takes.
        let s = mrb.str_new_static(b"borrowed");
        assert_eq!(s.len(), 8);
        assert_eq!(s.to_bytes(), b"borrowed".to_vec());
    }

    #[test]
    fn str_new_static_copies_on_in_place_write() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Appending reallocates the copy-on-write string before mutating,
        // so the in-place op yields the grown result without touching the
        // borrowed buffer.
        let s = mrb.str_new_static(b"static");
        s.cat(&mrb, b"+more")
            .expect("appending to a static-backed string succeeds after copy");
        assert_eq!(s.to_bytes(), b"static+more".to_vec());

        // Resize likewise reallocates first; shrinking drops the tail.
        let r = mrb.str_new_static(b"Hello, world!");
        r.resize(&mrb, 5)
            .expect("resizing a static-backed string succeeds");
        assert_eq!(r.to_bytes(), b"Hello".to_vec());
    }

    #[test]
    fn ary_new_capa_preallocates_an_empty_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Capacity is a hint, not content — the array starts empty and
        // fills as usual.
        let ary = mrb.ary_new_capa(8);
        assert!(ary.is_empty());
        ary.push(&mrb, mrb.str_new(b"x").as_value())
            .expect("push to a fresh array succeeds");
        assert_eq!(ary.len(), 1);
    }

    #[test]
    fn hash_new_capa_preallocates_an_empty_hash() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Capacity is a hint, not content — the hash starts empty and
        // fills as usual.
        let hash = mrb.hash_new_capa(8);
        assert!(hash.is_empty(&mrb));
        hash.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value(),
        )
        .expect("set to a fresh hash succeeds");
        assert_eq!(hash.len(&mrb), 1);
    }

    #[test]
    fn ary_new_from_values_copies_the_slice_in_order() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let values = [
            mrb.str_new(b"a").as_value(),
            mrb.str_new(b"b").as_value(),
            mrb.str_new(b"c").as_value(),
        ];
        let ary = mrb.ary_new_from_values(&values);

        assert_eq!(ary.len(), 3);
        assert_eq!(ary.entry(0).to_string(&mrb), "a");
        assert_eq!(ary.entry(2).to_string(&mrb), "c");
    }

    #[test]
    fn assoc_new_pairs_the_two_values_in_order() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let pair = mrb.assoc_new(
            mrb.str_new(b"car").as_value(),
            mrb.str_new(b"cdr").as_value(),
        );

        assert_eq!(pair.len(), 2);
        assert_eq!(pair.entry(0).to_string(&mrb), "car");
        assert_eq!(pair.entry(1).to_string(&mrb), "cdr");
    }
}
