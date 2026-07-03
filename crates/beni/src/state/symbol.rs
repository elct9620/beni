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

    /// `mrb_intern(mrb, name, len)` — intern a borrowed byte slice as a
    /// Symbol, creating it when absent. The general byte-taking intern:
    /// it interns the exact bytes the slice spans, so a name that embeds
    /// a NUL or is not NUL-terminated interns whole where `intern_cstr`
    /// would stop at the first NUL. mruby copies the bytes, so the borrow
    /// need not outlive the call (unlike `intern_static`).
    #[inline]
    pub fn intern(&self, name: &[u8]) -> Symbol {
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{Mrb, Symbol};

    #[test]
    fn intern_cstr_roundtrips_through_sym_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let sym = mrb.intern_cstr(c"beni_sym");

        assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
    }

    #[test]
    fn intern_str_yields_the_same_id_as_intern_cstr() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let via_str = mrb.intern_str(mrb.str_new(b"beni_sym").as_value());

        assert_eq!(via_str, mrb.intern_cstr(c"beni_sym"));
    }

    #[test]
    fn intern_interns_a_byte_slice_by_length_creating_the_symbol() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A runtime byte slice interns to a Symbol whose name round-trips,
        // and whose id equals interning the same name through the C-string
        // path — proving it's the same interned symbol.
        let sym = mrb.intern(b"beni_sym");
        assert_eq!(sym.name(&mrb).as_deref(), Some("beni_sym"));
        assert_eq!(sym.to_sym(), mrb.intern_cstr(c"beni_sym"));

        // It's length-based, not NUL-terminated: a slice carrying trailing
        // bytes past where a C string would stop interns those bytes too,
        // yielding a distinct symbol from the truncated name.
        let exact = b"abc";
        let padded = b"abc\0xyz";
        assert_eq!(
            mrb.intern(&exact[..]).name_bytes(&mrb).as_deref(),
            Some(&b"abc"[..])
        );
        assert_eq!(
            mrb.intern(&padded[..]).name_bytes(&mrb).as_deref(),
            Some(&b"abc\0xyz"[..])
        );
        assert_ne!(
            mrb.intern(&exact[..]).to_sym(),
            mrb.intern(&padded[..]).to_sym()
        );
    }

    #[test]
    fn intern_static_yields_the_same_id_as_the_copying_intern() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A `b"..."` literal is the `&'static [u8]` the no-copy intern
        // borrows; the name must read back and resolve to the same id the
        // copying intern produces, proving it's the same interned symbol.
        let sym = mrb.intern_static(b"beni_sym");

        assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
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
        assert_eq!(mrb.sym_name(sym).as_deref(), Some("\"a\\x00b\""));
        assert_eq!(mrb.sym_name_len(sym).as_deref(), Some(&b"a\0b"[..]));
    }

    #[test]
    fn intern_check_finds_an_interned_name_and_misses_an_uninterned_one() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A name no one has interned yet has no symbol, so the check misses.
        assert!(mrb.intern_check(b"beni_unseen").is_none());

        // Once the name is interned, the check finds it and reports the
        // same id the creating intern produced.
        let id = mrb.intern_cstr(c"beni_seen");
        assert_eq!(mrb.intern_check(b"beni_seen").map(Symbol::to_sym), Some(id));
    }

    #[test]
    fn sym_dump_quotes_a_non_identifier_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A plain identifier dumps bare; a name needing escaping is quoted.
        assert_eq!(
            mrb.sym_dump(mrb.intern_cstr(c"fred")).as_deref(),
            Some("fred")
        );
        assert_eq!(
            mrb.sym_dump(mrb.intern_str(mrb.str_new(b"a b").as_value()))
                .as_deref(),
            Some("\"a b\"")
        );
    }

    #[test]
    fn sym_name_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Two short names pack inline (<=4 packable chars), so mruby unpacks
        // each into one shared per-VM scratch buffer that the next name read
        // overwrites. Read both, holding the first across the second read.
        // A borrowed return would alias the buffer and show the first name
        // mutated to the second; the owned copy must stay intact.
        let first = mrb.sym_name(mrb.intern_cstr(c"aa")).expect("aa has a name");
        let second = mrb.sym_name(mrb.intern_cstr(c"bb")).expect("bb has a name");

        assert_eq!(first, "aa");
        assert_eq!(second, "bb");
    }

    #[test]
    fn sym_name_len_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Same aliasing net as `sym_name`: hold the first read's bytes
        // across a second inline-name read that overwrites the shared
        // scratch buffer; the owned copy must stay intact.
        let first = mrb
            .sym_name_len(mrb.intern_cstr(c"aa"))
            .expect("aa has a name");
        let second = mrb
            .sym_name_len(mrb.intern_cstr(c"bb"))
            .expect("bb has a name");

        assert_eq!(first, b"aa");
        assert_eq!(second, b"bb");
    }

    #[test]
    fn sym_dump_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Same aliasing net as `sym_name`: hold the first dump across a
        // second inline-name dump that overwrites the shared scratch
        // buffer; the owned copy must stay intact.
        let first = mrb.sym_dump(mrb.intern_cstr(c"aa")).expect("aa dumps");
        let second = mrb.sym_dump(mrb.intern_cstr(c"bb")).expect("bb dumps");

        assert_eq!(first, "aa");
        assert_eq!(second, "bb");
    }
}
