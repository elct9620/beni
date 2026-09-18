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

use crate::{Error, Mrb, Value};
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

/// Interning is canonical, so two symbols are equal exactly when they
/// name the same bytes. The id is what carries that, not the boxed
/// value, whose layout varies with the archive's boxing mode.
impl PartialEq for Symbol {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.to_sym() == other.to_sym()
    }
}

impl Eq for Symbol {}

impl core::hash::Hash for Symbol {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.to_sym().hash(state);
    }
}

/// The interned id, which is what distinguishes one symbol from another.
/// The name needs a live `Mrb` to read, so it is out of reach here.
impl core::fmt::Debug for Symbol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Symbol").field(&self.to_sym()).finish()
    }
}

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
    /// `Symbol::new`; the C-string intern `Mrb::intern_cstr` under
    /// another name, so a name too long to be a symbol surfaces as `Err`.
    #[inline]
    pub fn new(mrb: &Mrb, name: &core::ffi::CStr) -> Result<Self, Error> {
        mrb.intern_cstr(name)
    }

    /// Symbolize an id the caller has established this VM interned, via
    /// mruby's boxing-agnostic `mrb_symbol_value` constructor (an
    /// `MRB_INLINE` reached through bindgen's static-fn trampoline).
    /// Pure boxing — no `mrb_state` touched. The public crossing is
    /// `sys::FromRawId::from_raw`, which is `unsafe` for the
    /// establishing.
    #[inline]
    pub(crate) fn from_sym_unchecked(sym: sys::mrb_sym) -> Self {
        // SAFETY: `mrb_symbol_value` boxes a sym id and touches no
        // mrb_state; the resulting value is meaningful in the VM the
        // id was interned against, which the caller holds.
        Self(Value::from_raw_unchecked(unsafe {
            sys::mrb_symbol_value(sym)
        }))
    }

    /// The interned id this symbol carries, via the `mrb_symbol_func`
    /// shim — the `mrb_symbol` macro expanded inside the C compiler so
    /// the unbox matches the boxing config the linked archive was built
    /// with.
    #[inline]
    pub fn to_sym(self) -> sys::mrb_sym {
        // SAFETY: `self.0` is Symbol-tagged by the newtype's
        // construction contract; `mrb_symbol` reads only the value
        // payload and touches no mrb_state.
        unsafe { sys::mrb_symbol_func(self.0.as_raw()) }
    }

    /// The symbol's name as an owned `String`, via `Mrb::sym_name`.
    /// `None` when mruby yields a NULL name. A name
    /// carrying an embedded NUL comes back escaped to its quoted dump form;
    /// `name_bytes` reads the raw bytes — also the path for any non-UTF-8
    /// name, which mruby's escaping keeps unreachable here. A short name
    /// unpacks into a per-read scratch buffer the next name read overwrites,
    /// so the name is copied out rather than borrowed.
    #[inline]
    pub fn name(self, mrb: &Mrb) -> Option<String> {
        mrb.sym_name(self)
    }

    /// The symbol's raw name bytes as an owned `Vec<u8>`, via
    /// `Mrb::sym_name_len` — an embedded NUL preserved unescaped, where
    /// `name` returns the quoted dump form. `None` when mruby yields a NULL
    /// name. A short name unpacks into a per-read scratch buffer the next
    /// name read overwrites, so the bytes are copied out rather than
    /// borrowed.
    #[inline]
    pub fn name_bytes(self, mrb: &Mrb) -> Option<Vec<u8>> {
        mrb.sym_name_len(self)
    }

    /// The symbol's dump form as an owned `String`, via `Mrb::sym_dump`
    /// — the bare name for a plain identifier, otherwise the
    /// quoted and escaped form (Ruby's `Symbol#inspect` without the leading
    /// colon). `None` when mruby yields a NULL name. Reads without
    /// dispatching and never raises. A short name unpacks into a per-read
    /// scratch buffer the next name read overwrites, so the dump form is
    /// copied out rather than borrowed.
    #[inline]
    pub fn dump(self, mrb: &Mrb) -> Option<String> {
        mrb.sym_dump(self)
    }

    /// The symbol's name reified as an mruby String, via `mrb_sym_str`
    /// (Ruby's `Symbol#to_s`). Where `name`/`name_bytes`/`dump` copy the
    /// name into an owned Rust value, this yields a distinct, mutable
    /// `RString` the consumer owns — unfrozen, unlike `Symbol#name`. Builds
    /// the value without dispatching and never raises.
    #[inline]
    pub fn to_str(self, mrb: &Mrb) -> crate::RString {
        // SAFETY: `self.to_sym()` is interned against `mrb`, whose
        // pointer is live. `mrb_sym_str` reads the name and boxes a
        // String value; the result is String-tagged by construction.
        unsafe {
            crate::RString::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_sym_str(
                mrb.as_ptr(),
                self.to_sym(),
            )))
        }
    }
}

/// A name keying an operation — beni's mirror of `magnus`'s `IntoId`. A
/// string key interns to its symbol; an already-interned `Symbol` is
/// reused without re-interning. The typed surface accepts any `IntoSym`
/// wherever an operation is keyed by a name, routing every key through
/// mruby's `_id`-suffixed C variant, and hands a key that cannot
/// resolve back as its own `Err` before it acts.
///
/// The string keys differ in what an embedded NUL does: a `&CStr` key
/// names the bytes before its first NUL, a Rust string key names all of
/// its bytes.
pub trait IntoSym {
    /// Resolve this key to its `Symbol` against `mrb`, or the `Err` its
    /// intern surfaced.
    fn into_sym(self, mrb: &Mrb) -> Result<Symbol, Error>;
}

impl IntoSym for &core::ffi::CStr {
    /// Interns the bytes before the first NUL, so a name too long to be
    /// a symbol surfaces as `Err`.
    #[inline]
    fn into_sym(self, mrb: &Mrb) -> Result<Symbol, Error> {
        Symbol::new(mrb, self)
    }
}

impl IntoSym for &str {
    /// Interns all of the key's bytes, an embedded NUL included, so a
    /// name too long to be a symbol surfaces as `Err`.
    #[inline]
    fn into_sym(self, mrb: &Mrb) -> Result<Symbol, Error> {
        mrb.intern(self.as_bytes())
    }
}

impl IntoSym for String {
    /// Interns as the `&str` key does, for a name a caller owns.
    #[inline]
    fn into_sym(self, mrb: &Mrb) -> Result<Symbol, Error> {
        self.as_str().into_sym(mrb)
    }
}

impl IntoSym for Symbol {
    /// An already-interned `Symbol` reuses itself with no re-intern, so
    /// it always resolves.
    #[inline]
    fn into_sym(self, _mrb: &Mrb) -> Result<Symbol, Error> {
        Ok(self)
    }
}
