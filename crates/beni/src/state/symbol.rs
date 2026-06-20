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

    /// `mrb_intern_static(mrb, name, len)` — intern `name` as a Symbol
    /// without copying its bytes, the no-copy counterpart of `intern_cstr`
    /// / `intern_str`. mruby keeps the borrowed pointer and never frees it,
    /// so the buffer must outlive the VM; the `'static` bound is what makes
    /// this safe. A `b"..."` literal is a `&'static [u8]`, so this also
    /// serves mruby's `mrb_intern_lit` convenience.
    #[inline]
    pub fn intern_static(&self, name: &'static [u8]) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `name` is `'static`, so the borrowed
            // buffer outlives the VM as mruby's no-free intern requires.
            unsafe {
                sys::mrb_intern_static(
                    self.as_ptr(),
                    name.as_ptr() as *const core::ffi::c_char,
                    name.len(),
                )
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_sym_name(mrb, sym)` — return the C string name of `sym`,
    /// or `None` if mruby yields a NULL pointer (e.g. uninterned id).
    /// The returned slice points into mruby's interned string storage
    /// and lives for the duration of the VM. A name carrying an embedded
    /// NUL comes back escaped to its quoted dump form; `Mrb::sym_name_len`
    /// reads the raw bytes.
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

    /// `mrb_sym_name_len(mrb, sym, &len)` — return the raw name bytes of
    /// `sym`, its true length carried out of band so an embedded NUL is
    /// preserved unescaped rather than driving `Mrb::sym_name` to its
    /// quoted dump form. `None` when mruby yields a NULL pointer (e.g.
    /// uninterned id). The slice points into mruby's interned string
    /// storage and lives for the duration of the VM.
    #[inline]
    pub fn sym_name_len(&self, sym: sys::mrb_sym) -> Option<&'static [u8]> {
        #[cfg(mruby_linked)]
        {
            let mut len: sys::mrb_int = 0;
            // SAFETY: `self` is alive; `&mut len` is a valid out-pointer.
            let ptr = unsafe { sys::mrb_sym_name_len(self.as_ptr(), sym, &mut len) };
            if ptr.is_null() {
                return None;
            }
            // SAFETY: mruby reports a non-negative length for a live name
            // and the storage lives for the VM's duration, which the
            // caller upholds via the owning `Mrb`; the `'static` slice is
            // sound for that lifetime.
            Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }

    /// `mrb_sym_dump(mrb, sym)` — return the dump form of `sym`'s name:
    /// the bare name for a plain identifier, otherwise the quoted and
    /// escaped form (Ruby's `Symbol#inspect` without the leading colon).
    /// `None` when mruby yields a NULL pointer (e.g. uninterned id).
    /// Reads without dispatching and never raises.
    #[inline]
    pub fn sym_dump(&self, sym: sys::mrb_sym) -> Option<&'static str> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive.
            let ptr = unsafe { sys::mrb_sym_dump(self.as_ptr(), sym) };
            if ptr.is_null() {
                return None;
            }
            // SAFETY: the dump string lives for the VM's duration, which
            // the caller upholds via the owning `Mrb`.
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

        let via_str = mrb.intern_str(mrb.str_new(b"beni_sym").as_value());

        assert_eq!(via_str, mrb.intern_cstr(c"beni_sym"));
    }

    #[test]
    fn intern_static_yields_the_same_id_as_the_copying_intern() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A `b"..."` literal is the `&'static [u8]` the no-copy intern
        // borrows; the name must read back and resolve to the same id the
        // copying intern produces, proving it's the same interned symbol.
        let sym = mrb.intern_static(b"beni_sym");

        assert_eq!(mrb.sym_name(sym), Some("beni_sym"));
        assert_eq!(sym, mrb.intern_cstr(c"beni_sym"));
    }

    #[test]
    fn sym_name_len_carries_the_full_bytes_past_an_embedded_nul() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Intern a name with an embedded NUL through the bytes path; the
        // C-string `sym_name` would stop at the NUL, the length-carrying
        // read keeps the full bytes.
        let sym = mrb.intern_str(mrb.str_new(b"a\0b").as_value());

        // `sym_name` escapes a NUL-containing name to its dump form; only
        // the length-carrying read returns the raw bytes.
        assert_eq!(mrb.sym_name(sym), Some("\"a\\x00b\""));
        assert_eq!(mrb.sym_name_len(sym), Some(&b"a\0b"[..]));
    }

    #[test]
    fn sym_dump_quotes_a_non_identifier_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A plain identifier dumps bare; a name needing escaping is quoted.
        assert_eq!(mrb.sym_dump(mrb.intern_cstr(c"fred")), Some("fred"));
        assert_eq!(
            mrb.sym_dump(mrb.intern_str(mrb.str_new(b"a b").as_value())),
            Some("\"a b\"")
        );
    }
}
