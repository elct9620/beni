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
    /// mruby yields a NULL name. The slice points into mruby's interned
    /// storage, which lives for the VM's duration.
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
    fn from_value_discriminates_the_symbol_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let sym_val = Symbol::new(&mrb, c"k").into_value(&mrb);

        assert!(Symbol::from_value(sym_val).is_some());
        // A non-symbol value — and an immediate — both reject.
        assert!(Symbol::from_value(mrb.str_new(b"k")).is_none());
        assert!(Symbol::from_value(42i32.into_value(&mrb)).is_none());
    }
}
