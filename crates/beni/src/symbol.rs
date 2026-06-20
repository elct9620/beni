//! Typed `Symbol` newtype around a Symbol-tagged `Value` — beni's
//! mirror of magnus's `Symbol`.
//!
//! `Symbol` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Symbol` is exactly an `mrb_value` known to carry
//! an mruby Symbol.
//!
//! Construct from a name (`Symbol::new`, which interns) or from an
//! already-interned id (`Symbol::from_sym`); read the interned id back
//! with `to_sym` and the name with `name`. The checked `Value` →
//! `Symbol` downcast lives on `FromValue`, the `Symbol` → `Value`
//! boxing on `IntoValue`, alongside the other conversions.

use crate::{Mrb, Value};
use beni_sys as sys;

/// Typed handle on an mruby `Symbol`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Symbol::new` (intern a name), `Symbol::from_sym`
/// (box an interned id), the checked `FromValue` downcast
/// (`Symbol::from_value`, tag-discriminated), or
/// `Symbol::from_value_unchecked`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Symbol(Value);

impl Symbol {
    /// Wrap a `Value` the caller has already determined to be
    /// Symbol-tagged.
    ///
    /// # Safety
    ///
    /// `v` must be Symbol-tagged. Operating on a non-Symbol value
    /// through this newtype is undefined (the unbox assumes Symbol
    /// payload).
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
    /// yet migrated.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// Intern `name` and symbolize it. Counterpart to magnus's
    /// `Symbol::new`; equivalent to symbolizing `mrb.intern_cstr(name)`.
    #[inline]
    pub fn new(mrb: &Mrb, name: &core::ffi::CStr) -> Self {
        #[cfg(mruby_linked)]
        {
            Self::from_sym(mrb.intern_cstr(name))
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name);
            crate::not_linked()
        }
    }

    /// Symbolize an already-interned id via mruby's boxing-agnostic
    /// `mrb_symbol_value` constructor (an `MRB_INLINE` reached through
    /// bindgen's static-fn trampoline). Pure boxing — no `mrb_state`
    /// touched — so the caller keeps the id's originating VM in scope.
    #[inline]
    pub fn from_sym(sym: sys::mrb_sym) -> Self {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb_symbol_value` boxes a sym id and touches no
            // mrb_state; the resulting value is meaningful in the VM the
            // id was interned against, which the caller holds.
            Self(Value::from_raw(unsafe { sys::mrb_symbol_value(sym) }))
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }

    /// The interned id this symbol carries, via the `mrb_symbol_func`
    /// shim — the `mrb_symbol` macro expanded inside the C compiler so
    /// the unbox matches the boxing config the linked archive was built
    /// with.
    #[inline]
    pub fn to_sym(self) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.0` is Symbol-tagged by the newtype's
            // construction contract; `mrb_symbol` reads only the value
            // payload and touches no mrb_state.
            unsafe { sys::mrb_symbol_func(self.0.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// The symbol's name, via `to_sym` + `Mrb::sym_name`. `None` when
    /// mruby yields a NULL name. A name carrying an embedded NUL comes
    /// back escaped to its quoted dump form; `name_bytes` reads the raw
    /// bytes. The slice points into mruby's interned storage, which lives
    /// for the VM's duration.
    #[inline]
    pub fn name(self, mrb: &Mrb) -> Option<&'static str> {
        #[cfg(mruby_linked)]
        {
            mrb.sym_name(self.to_sym())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// The symbol's raw name bytes, via `to_sym` + `Mrb::sym_name_len` —
    /// an embedded NUL preserved unescaped, where `name` returns the
    /// quoted dump form. `None` when mruby yields a NULL name.
    #[inline]
    pub fn name_bytes(self, mrb: &Mrb) -> Option<&'static [u8]> {
        #[cfg(mruby_linked)]
        {
            mrb.sym_name_len(self.to_sym())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// The symbol's dump form, via `to_sym` + `Mrb::sym_dump` — the bare
    /// name for a plain identifier, otherwise the quoted and escaped form
    /// (Ruby's `Symbol#inspect` without the leading colon). `None` when
    /// mruby yields a NULL name. Reads without dispatching and never raises.
    #[inline]
    pub fn dump(self, mrb: &Mrb) -> Option<&'static str> {
        #[cfg(mruby_linked)]
        {
            mrb.sym_dump(self.to_sym())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// The symbol's name reified as an mruby String, via `mrb_sym_str`
    /// (Ruby's `Symbol#to_s`). Where `name`/`name_bytes`/`dump` borrow into
    /// mruby's interned storage, this yields a distinct, mutable `RString`
    /// the consumer owns — unfrozen, unlike `Symbol#name`. Builds the value
    /// without dispatching and never raises.
    #[inline]
    pub fn to_str(self, mrb: &Mrb) -> crate::RString {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.to_sym()` is interned against `mrb`, whose
            // pointer is live. `mrb_sym_str` reads the name and boxes a
            // String value; the result is String-tagged by construction.
            unsafe {
                crate::RString::from_value_unchecked(Value::from_raw(sys::mrb_sym_str(
                    mrb.as_ptr(),
                    self.to_sym(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}

/// A definition or lookup name given as a symbol-or-name key — beni's
/// mirror of `magnus`'s `IntoId`. A name interns to its symbol; an
/// already-interned `Symbol` is reused without re-interning. The typed
/// define/get surface accepts any `IntoSym`, routing every key through
/// mruby's `_id`-suffixed C variant.
pub trait IntoSym {
    /// Resolve this key to its interned `mrb_sym` against `mrb`.
    fn into_sym(self, mrb: &Mrb) -> sys::mrb_sym;
}

impl IntoSym for &core::ffi::CStr {
    /// A name key interns through `Mrb::intern_cstr`.
    #[inline]
    fn into_sym(self, mrb: &Mrb) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            mrb.intern_cstr(self)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}

impl IntoSym for Symbol {
    /// An already-interned `Symbol` reuses its id with no re-intern.
    #[inline]
    fn into_sym(self, _mrb: &Mrb) -> sys::mrb_sym {
        #[cfg(mruby_linked)]
        {
            self.to_sym()
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;
    use crate::{FromValue, IntoValue};

    #[test]
    fn name_sym_and_rebuild_roundtrip() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let sym = Symbol::new(&mrb, c"flags");

        assert_eq!(sym.name(&mrb), Some("flags"));
        // The unboxed id must equal interning the same name — a wrong
        // boxing shift in the unbox shim would diverge here.
        assert_eq!(sym.to_sym(), mrb.intern_cstr(c"flags"));
        // Re-boxing the id yields an equal symbol.
        assert_eq!(Symbol::from_sym(sym.to_sym()).name(&mrb), Some("flags"));
    }

    #[test]
    fn name_bytes_and_dump_read_the_symbol_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A plain identifier: bytes equal the name, dump is bare.
        let plain = Symbol::new(&mrb, c"fred");
        assert_eq!(plain.name_bytes(&mrb), Some(&b"fred"[..]));
        assert_eq!(plain.dump(&mrb), Some("fred"));

        // An embedded NUL: `name` escapes it to the dump form, only
        // `name_bytes` returns the raw bytes.
        let nul = Symbol::from_sym(mrb.intern_str(mrb.str_new(b"a\0b").as_value()));
        assert_eq!(nul.name(&mrb), Some("\"a\\x00b\""));
        assert_eq!(nul.name_bytes(&mrb), Some(&b"a\0b"[..]));

        // A name needing escaping dumps quoted.
        let spaced = Symbol::from_sym(mrb.intern_str(mrb.str_new(b"a b").as_value()));
        assert_eq!(spaced.dump(&mrb), Some("\"a b\""));
    }

    #[test]
    fn to_str_reifies_the_name_as_a_mutable_string() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let sym = Symbol::new(&mrb, c"flags");

        // The reified String carries the symbol's name bytes verbatim.
        let str = sym.to_str(&mrb);
        assert_eq!(str.to_bytes(), b"flags");

        // It is `Symbol#to_s`, not `#name`: the value is unfrozen, so a
        // consumer may mutate it — `Symbol#name` would come back frozen.
        assert!(!str
            .as_value()
            .funcall(&mrb, c"frozen?", &[])
            .expect("frozen? does not raise")
            .to_bool());
    }

    #[test]
    fn to_sym_coerces_symbol_string_and_rejects_others() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A symbol value coerces to the same symbol.
        let sym = Symbol::new(&mrb, c"key");
        let from_sym = sym.as_value().to_sym(&mrb).expect("a symbol value coerces");
        assert_eq!(from_sym.to_sym(), sym.to_sym());

        // A string value interns to the symbol of its contents — the id
        // matches interning the same name directly.
        let from_str = mrb
            .str_new(b"key")
            .as_value()
            .to_sym(&mrb)
            .expect("a string value coerces");
        assert_eq!(from_str.to_sym(), mrb.intern_cstr(c"key"));

        // A value that is neither a symbol nor a string rejects.
        assert!(42i32.into_value(&mrb).to_sym(&mrb).is_err());
    }

    #[test]
    fn from_value_discriminates_the_symbol_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let sym_val = Symbol::new(&mrb, c"k").into_value(&mrb);

        assert!(Symbol::from_value(sym_val).is_some());
        // A non-symbol value — and an immediate — both reject.
        assert!(Symbol::from_value(mrb.str_new(b"k").as_value()).is_none());
        assert!(Symbol::from_value(42i32.into_value(&mrb)).is_none());
    }
}
