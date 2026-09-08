//! Symbol intern + lookup on `Mrb`.
//!
//! Inherent methods that turn a name (NUL-terminated `&CStr` or
//! arbitrary bytes via an `mrb_value` String) into an `mrb_sym`, or
//! read the C-string name back from a symbol id.

use crate::{Mrb, Symbol, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_intern_cstr(mrb, s)` — intern a NUL-terminated C string
    /// as a Symbol id.
    #[inline]
    pub fn intern_cstr(&self, s: &core::ffi::CStr) -> sys::mrb_sym {
        // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
        // the `&CStr` contract.
        unsafe { sys::mrb_intern_cstr(self.as_ptr(), s.as_ptr()) }
    }

    /// `mrb_intern_str(mrb, str)` — intern the bytes of an mruby
    /// String value as a Symbol. Use this when the name arrives as
    /// arbitrary bytes that may not be NUL-safe; otherwise prefer
    /// `Mrb::intern_cstr`.
    #[inline]
    pub fn intern_str(&self, s: Value) -> sys::mrb_sym {
        // SAFETY: `self` is alive; `s` originates from the same VM.
        unsafe { sys::mrb_intern_str(self.as_ptr(), s.as_raw()) }
    }

    /// `mrb_intern(mrb, name, len)` — intern a borrowed byte slice as a
    /// Symbol, creating it when absent. The general byte-taking intern:
    /// it interns the exact bytes the slice spans, so a name that embeds
    /// a NUL or is not NUL-terminated interns whole where `intern_cstr`
    /// would stop at the first NUL. mruby copies the bytes, so the borrow
    /// need not outlive the call (unlike `intern_static`).
    #[inline]
    pub fn intern(&self, name: &[u8]) -> Symbol {
        // SAFETY: `self` is alive; `name` is a valid byte slice and its
        // length is passed alongside, so the borrow need not be NUL-safe.
        let sym = unsafe {
            sys::mrb_intern(
                self.as_ptr(),
                name.as_ptr() as *const core::ffi::c_char,
                name.len(),
            )
        };
        Symbol::from_sym(sym)
    }

    /// `mrb_intern_static(mrb, name, len)` — intern `name` as a Symbol
    /// without copying its bytes, the no-copy counterpart of `intern_cstr`
    /// / `intern_str`. mruby keeps the borrowed pointer and never frees it,
    /// so the buffer must outlive the VM; the `'static` bound is what makes
    /// this safe. A `b"..."` literal is a `&'static [u8]`, so this also
    /// serves mruby's `mrb_intern_lit` convenience.
    #[inline]
    pub fn intern_static(&self, name: &'static [u8]) -> sys::mrb_sym {
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

    /// `mrb_intern_check(mrb, name, len)` — the non-creating counterpart
    /// of the interns: `Some` Symbol when `name`'s bytes are already
    /// interned, `None` when no such symbol exists. A presence test that
    /// dispatches nothing and never raises, leaving the symbol table
    /// untouched. mruby reserves id 0 for "not interned", so a zero result
    /// maps to `None`. This is the byte-taking primitive mruby's
    /// `mrb_intern_check_cstr` (NUL-terminated) and `mrb_intern_check_str`
    /// (an mruby String value) both forward to.
    #[inline]
    pub fn intern_check(&self, name: &[u8]) -> Option<Symbol> {
        // SAFETY: `self` is alive; `name` is a valid byte slice and its
        // length is passed alongside, so the borrow need not be NUL-safe.
        let sym = unsafe {
            sys::mrb_intern_check(
                self.as_ptr(),
                name.as_ptr() as *const core::ffi::c_char,
                name.len(),
            )
        };
        (sym != 0).then(|| Symbol::from_sym(sym))
    }

    /// `mrb_sym_name(mrb, sym)` — return the name of `sym` as an owned
    /// `String`, or `None` if mruby yields a NULL pointer (e.g. uninterned
    /// id). A short symbol name unpacks into a per-read scratch buffer the
    /// next name read overwrites, so the bytes are copied out before this
    /// returns rather than borrowed. A name carrying an embedded NUL comes
    /// back escaped to its quoted dump form; `Mrb::sym_name_len` reads the
    /// raw bytes. mruby escapes non-identifier names into a quoted ASCII
    /// form, so a name is always valid UTF-8; the empty-string fallback on
    /// a non-UTF-8 name is defensive — reach for `Mrb::sym_name_len` to
    /// read raw bytes.
    #[inline]
    pub fn sym_name(&self, sym: sys::mrb_sym) -> Option<String> {
        // SAFETY: `self` is alive.
        let ptr = unsafe { sys::mrb_sym_name(self.as_ptr(), sym) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: `ptr` is a valid C string for the duration of this
        // call; copy its bytes out at once, before the next name read
        // can overwrite the scratch buffer a short name unpacks into.
        Some(
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or("")
                .to_owned(),
        )
    }

    /// `mrb_sym_name_len(mrb, sym, &len)` — return the raw name bytes of
    /// `sym` as an owned `Vec<u8>`, its true length carried out of band so
    /// an embedded NUL is preserved unescaped rather than driving
    /// `Mrb::sym_name` to its quoted dump form. `None` when mruby yields a
    /// NULL pointer (e.g. uninterned id). A short symbol name unpacks into
    /// a per-read scratch buffer the next name read overwrites, so the
    /// bytes are copied out before this returns rather than borrowed.
    #[inline]
    pub fn sym_name_len(&self, sym: sys::mrb_sym) -> Option<Vec<u8>> {
        let mut len: sys::mrb_int = 0;
        // SAFETY: `self` is alive; `&mut len` is a valid out-pointer.
        let ptr = unsafe { sys::mrb_sym_name_len(self.as_ptr(), sym, &mut len) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: mruby reports a non-negative length for a live name,
        // and `ptr` spans that many valid bytes for the duration of
        // this call; copy them out at once, before the next name read
        // can overwrite the scratch buffer a short name unpacks into.
        Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) }.to_vec())
    }

    /// `mrb_sym_dump(mrb, sym)` — return the dump form of `sym`'s name as
    /// an owned `String`: the bare name for a plain identifier, otherwise
    /// the quoted and escaped form (Ruby's `Symbol#inspect` without the
    /// leading colon). `None` when mruby yields a NULL pointer (e.g.
    /// uninterned id). Reads without dispatching and never raises. The dump
    /// form draws on the same per-read scratch buffer a short name unpacks
    /// into, so the bytes are copied out before this returns rather than
    /// borrowed. The dump form is always ASCII, so the empty-string
    /// fallback on a non-UTF-8 name is defensive and unreachable.
    #[inline]
    pub fn sym_dump(&self, sym: sys::mrb_sym) -> Option<String> {
        // SAFETY: `self` is alive.
        let ptr = unsafe { sys::mrb_sym_dump(self.as_ptr(), sym) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: `ptr` is a valid C string for the duration of this
        // call; copy its bytes out at once, before the next name read
        // can overwrite the scratch buffer a short name unpacks into.
        Some(
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or("")
                .to_owned(),
        )
    }
}
