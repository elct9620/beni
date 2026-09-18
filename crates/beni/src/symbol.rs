//! The interned id and the symbol value that boxes it — beni's mirror of
//! magnus's `Id` and `Symbol`.
//!
//! `Id` carries the `mrb_sym` itself, which is not a value; `Symbol` is
//! `#[repr(transparent)]` over `Value`, an `mrb_value` known to carry an
//! mruby Symbol. The two convert into each other with `From`, boxing or
//! unboxing without touching an interpreter. Names intern to an `Id`;
//! `Symbol::new` interns and boxes in one step. The checked `Value` →
//! `Symbol` downcast lives on `FromValue`, the boxing on `IntoValue`,
//! alongside the other conversions.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

/// An interned symbol id — magnus's `Id`. Compares and hashes by the id,
/// which interning makes canonical, so two ids are equal exactly when
/// they name the same bytes.
///
/// Construct by interning a name (`Mrb::intern` and its siblings), from
/// a `Symbol`, or across the raw seam through `sys::FromRawId`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Id(sys::mrb_sym);

impl Id {
    /// Wrap an id this VM just produced. The public crossing is
    /// `sys::FromRawId::from_raw`, which is `unsafe` for the
    /// establishing.
    #[inline]
    pub(crate) const fn from_raw_unchecked(sym: sys::mrb_sym) -> Self {
        Self(sym)
    }

    /// The raw id, for the crate's own calls into `beni::sys`.
    #[inline]
    pub(crate) const fn to_raw(self) -> sys::mrb_sym {
        self.0
    }
}

/// Typed handle on an mruby `Symbol`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Symbol::new` (intern a name), `Symbol::from` an `Id`,
/// the checked `FromValue` downcast (`Symbol::from_value`,
/// tag-discriminated), or `Symbol::from_value_unchecked`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Symbol(Value);

impl From<Id> for Symbol {
    /// Box the id through mruby's boxing-agnostic `mrb_symbol_value`
    /// constructor (an `MRB_INLINE` reached through bindgen's static-fn
    /// trampoline), touching no `mrb_state`.
    #[inline]
    fn from(id: Id) -> Self {
        // SAFETY: `mrb_symbol_value` boxes a sym id and touches no
        // mrb_state; the value is meaningful in the VM the id belongs to.
        Self(Value::from_raw_unchecked(unsafe {
            sys::mrb_symbol_value(id.0)
        }))
    }
}

impl From<Symbol> for Id {
    /// Unbox through the `mrb_symbol_func` shim — the `mrb_symbol` macro
    /// expanded inside the C compiler so the unbox matches the boxing
    /// config the linked archive was built with.
    #[inline]
    fn from(sym: Symbol) -> Self {
        // SAFETY: `sym.0` is Symbol-tagged by the newtype's construction
        // contract; `mrb_symbol` reads only the value payload.
        Self(unsafe { sys::mrb_symbol_func(sym.0.as_raw()) })
    }
}

/// Compared by the id rather than the boxed value, whose layout varies
/// with the archive's boxing mode.
impl PartialEq for Symbol {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Id::from(*self) == Id::from(*other)
    }
}

impl Eq for Symbol {}

impl PartialEq<Id> for Symbol {
    #[inline]
    fn eq(&self, other: &Id) -> bool {
        Id::from(*self) == *other
    }
}

impl PartialEq<Symbol> for Id {
    #[inline]
    fn eq(&self, other: &Symbol) -> bool {
        *self == Id::from(*other)
    }
}

/// The interned id, which is what distinguishes one symbol from another.
/// The name needs a live `Mrb` to read, so it is out of reach here.
impl core::fmt::Debug for Symbol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Symbol").field(&Id::from(*self)).finish()
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
        mrb.intern_cstr(name).map(Self::from)
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
        mrb.sym_name(self.into())
    }

    /// The symbol's raw name bytes as an owned `Vec<u8>`, via
    /// `Mrb::sym_name_len` — an embedded NUL preserved unescaped, where
    /// `name` returns the quoted dump form. `None` when mruby yields a NULL
    /// name. A short name unpacks into a per-read scratch buffer the next
    /// name read overwrites, so the bytes are copied out rather than
    /// borrowed.
    #[inline]
    pub fn name_bytes(self, mrb: &Mrb) -> Option<Vec<u8>> {
        mrb.sym_name_len(self.into())
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
        mrb.sym_dump(self.into())
    }

    /// The symbol's name reified as an mruby String, via `mrb_sym_str`
    /// (Ruby's `Symbol#to_s`). Where `name`/`name_bytes`/`dump` copy the
    /// name into an owned Rust value, this yields a distinct, mutable
    /// `RString` the consumer owns — unfrozen, unlike `Symbol#name`. Builds
    /// the value without dispatching and never raises.
    #[inline]
    pub fn to_str(self, mrb: &Mrb) -> crate::RString {
        // SAFETY: `self`'s id is interned against `mrb`, whose
        // pointer is live. `mrb_sym_str` reads the name and boxes a
        // String value; the result is String-tagged by construction.
        unsafe {
            crate::RString::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_sym_str(
                mrb.as_ptr(),
                Id::from(self).0,
            )))
        }
    }
}

/// A name keying an operation — beni's mirror of `magnus`'s `IntoId`.
/// Every key resolves to the `Id` it names: a string key by interning, an
/// `Id` or `Symbol` as the id it already is. The typed surface accepts
/// any `IntoId` wherever an operation is keyed by a name, routing every
/// key through mruby's `_id`-suffixed C variant, and hands a key that
/// cannot resolve back as its own `Err` before it acts.
///
/// The string keys differ in what an embedded NUL does: a `&CStr` key
/// names the bytes before its first NUL, a Rust string key names all of
/// its bytes.
pub trait IntoId {
    /// Resolve this key to its `Id` against `mrb`, or the `Err` its
    /// intern surfaced.
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error>;
}

impl IntoId for &core::ffi::CStr {
    /// Interns the bytes before the first NUL, so a name too long to be
    /// a symbol surfaces as `Err`.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        mrb.intern_cstr(self)
    }
}

impl IntoId for &str {
    /// Interns all of the key's bytes, an embedded NUL included, so a
    /// name too long to be a symbol surfaces as `Err`.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        mrb.intern(self.as_bytes())
    }
}

impl IntoId for String {
    /// Interns as the `&str` key does, for a name a caller owns.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        self.as_str().into_id(mrb)
    }
}

impl IntoId for Id {
    /// Already interned, so it always resolves.
    #[inline]
    fn into_id(self, _mrb: &Mrb) -> Result<Id, Error> {
        Ok(self)
    }
}

impl IntoId for Symbol {
    /// Resolves to the id it boxes, with no re-intern.
    #[inline]
    fn into_id(self, _mrb: &Mrb) -> Result<Id, Error> {
        Ok(self.into())
    }
}
