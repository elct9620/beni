//! String / Array / Hash factories on `Mrb`.
//!
//! `str_new` / `str_new_cstr` construct mruby Strings from Rust byte
//! slices or a NUL-terminated `&CStr`. `ary_new` / `hash_new` return
//! typed `Array` / `Hash` newtypes — per-collection operations
//! (`push`, `set`, `get`, `keys`) live on the value newtype rather
//! than on `Mrb` so the call shape mirrors Ruby (`arr.push(x)`,
//! not `mrb.ary_push(arr, x)`).

use crate::{Array, Hash, Mrb, Value};
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
    pub fn str_new(&self, bytes: &[u8]) -> Value {
        #[cfg(mruby_linked)]
        {
            let len = bytes.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `self` is alive by the `&self` borrow; `bytes`
            // outlives the synchronous call.
            Value::from_raw(unsafe {
                sys::mrb_str_new(
                    self.as_ptr(),
                    bytes.as_ptr() as *const core::ffi::c_char,
                    len,
                )
            })
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
    pub fn str_new_cstr(&self, s: &core::ffi::CStr) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
            // the `&CStr` contract.
            Value::from_raw(unsafe { sys::mrb_str_new_cstr(self.as_ptr(), s.as_ptr()) })
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
