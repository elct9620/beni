//! Symbol intern + lookup on `Mrb`.
//!
//! Inherent methods that turn a name (NUL-terminated `&CStr` or
//! arbitrary bytes via an `mrb_value` String) into an `mrb_sym`, or
//! read the C-string name back from a symbol id.

use crate::{Mrb, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_intern_cstr(mrb, s)` — intern a NUL-terminated C string
    /// as a Symbol id.
    #[inline]
    pub fn intern_cstr(&self, s: &core::ffi::CStr) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
            // the `&CStr` contract.
            unsafe { sys::mrb_intern_cstr(self.as_ptr(), s.as_ptr()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = s;
            crate::not_linked()
        }
    }

    /// `mrb_intern_str(mrb, str)` — intern the bytes of an mruby
    /// String value as a Symbol. Use this when the name arrives as
    /// arbitrary bytes that may not be NUL-safe; otherwise prefer
    /// `Mrb::intern_cstr`.
    #[inline]
    pub fn intern_str(&self, s: Value) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `s` originates from the same VM.
            unsafe { sys::mrb_intern_str(self.as_ptr(), s.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = s;
            crate::not_linked()
        }
    }

    /// `mrb_sym_name(mrb, sym)` — return the C string name of `sym`,
    /// or `None` if mruby yields a NULL pointer (e.g. uninterned id).
    /// The returned slice points into mruby's interned string storage
    /// and lives for the duration of the VM.
    #[inline]
    pub fn sym_name(&self, sym: sys::mrb_sym) -> Option<&'static str> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive.
            let ptr = unsafe { sys::mrb_sym_name(self.as_ptr(), sym) };
            if ptr.is_null() {
                return None;
            }
            // SAFETY: mruby's interned symbol storage lives for the
            // duration of the VM; treating the slice as `'static` is
            // sound for that lifetime, which the caller upholds via the
            // owning `Mrb`.
            Some(
                unsafe { core::ffi::CStr::from_ptr(ptr) }
                    .to_str()
                    .unwrap_or(""),
            )
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::Mrb;

    #[test]
    fn intern_cstr_roundtrips_through_sym_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let sym = mrb.intern_cstr(c"beni_sym");

        assert_eq!(mrb.sym_name(sym), Some("beni_sym"));
    }

    #[test]
    fn intern_str_yields_the_same_id_as_intern_cstr() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let via_str = mrb.intern_str(mrb.str_new(b"beni_sym"));

        assert_eq!(via_str, mrb.intern_cstr(c"beni_sym"));
    }
}
