//! String / Array / Hash factories on `Mrb`.
//!
//! `str_new` / `str_new_cstr` construct mruby Strings from Rust byte
//! slices or a NUL-terminated `&CStr`. `ary_new` / `hash_new` return
//! typed `Array` / `Hash` newtypes — per-collection operations
//! (`push`, `set`, `get`, `keys`) live on the value newtype rather
//! than on `Mrb` so the call shape mirrors Ruby (`arr.push(x)`,
//! not `mrb.ary_push(arr, x)`).

use crate::{Array, Hash, Mrb, RString, Value};
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
    pub fn ary_new_from(&self, values: &[Value]) -> Array {
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
    fn ary_new_capa_preallocates_an_empty_array() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Capacity is a hint, not content — the array starts empty and
        // fills as usual.
        let ary = mrb.ary_new_capa(8);
        assert!(ary.is_empty());
        ary.push(&mrb, mrb.str_new(b"x").as_value());
        assert_eq!(ary.len(), 1);
    }

    #[test]
    fn ary_new_from_copies_the_slice_in_order() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let values = [
            mrb.str_new(b"a").as_value(),
            mrb.str_new(b"b").as_value(),
            mrb.str_new(b"c").as_value(),
        ];
        let ary = mrb.ary_new_from(&values);

        assert_eq!(ary.len(), 3);
        assert_eq!(ary.entry(0).to_string(&mrb), "a");
        assert_eq!(ary.entry(2).to_string(&mrb), "c");
    }
}
