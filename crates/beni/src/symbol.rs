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

use crate::{sys::AsRawValue, Error, IntoValue, Mrb, Value};
use beni_sys as sys;

mod namespace;

use namespace::Sealed;
pub(crate) use namespace::{key, Namespace};
pub use namespace::{ConstId, CvarId, GvarId, IvarId};

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

    /// `mrb_sym_name(mrb, id)` — the id's name as an owned `String`.
    /// Surfaces an `Err` carrying an `ArgumentError` for a name that is not
    /// UTF-8, as magnus's `Id::name` errs, with mruby's wording for invalid
    /// UTF-8; mruby has no `EncodingError`. Copies where magnus borrows a
    /// `&'static str`: a short symbol name unpacks into a per-read scratch
    /// buffer the next name read overwrites (`mrb_sym_name_len`,
    /// `vendor/mruby/src/symbol.c`). A name carrying an embedded NUL comes
    /// back escaped to its quoted dump form; `Id::name_bytes` reads the raw
    /// bytes.
    #[inline]
    pub fn name(self, mrb: &Mrb) -> Result<String, Error> {
        // SAFETY: `mrb` is alive.
        let ptr = unsafe { sys::mrb_sym_name(mrb.as_ptr(), self.to_raw()) };
        // SAFETY: a non-NULL `ptr` is a valid C string for the duration of
        // this call; it is copied out at once, before the next name read can
        // overwrite the scratch buffer a short name unpacks into.
        let name = unsafe { self.name_cstr(mrb, ptr) }?;
        name.to_str()
            .map(str::to_owned)
            .map_err(|_| crate::try_convert::invalid_utf8(mrb))
    }

    /// `mrb_sym_name_len(mrb, id, &len)` — the id's raw name bytes as an
    /// owned `Vec<u8>`, its true length carried out of band so an embedded
    /// NUL is preserved unescaped rather than driving `Id::name` to its
    /// quoted dump form. A short symbol name unpacks into a per-read scratch
    /// buffer the next name read overwrites, so the bytes are copied out
    /// before this returns rather than borrowed.
    #[inline]
    pub fn name_bytes(self, mrb: &Mrb) -> Result<Vec<u8>, Error> {
        let mut len: sys::mrb_int = 0;
        // SAFETY: `mrb` is alive; `&mut len` is a valid out-pointer.
        let ptr = unsafe { sys::mrb_sym_name_len(mrb.as_ptr(), self.to_raw(), &mut len) };
        if ptr.is_null() {
            return Err(unnamed(mrb));
        }
        // SAFETY: mruby reports a non-negative length for a live name,
        // and `ptr` spans that many valid bytes for the duration of
        // this call; copy them out at once, before the next name read
        // can overwrite the scratch buffer a short name unpacks into.
        Ok(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) }.to_vec())
    }

    /// `mrb_sym_dump(mrb, id)` — the dump form of the id's name as an
    /// owned `String`: the bare name for a plain identifier, otherwise
    /// the quoted and escaped form (Ruby's `Symbol#inspect` without the
    /// leading colon), which escapes every byte outside printable ASCII.
    /// Reads without dispatching. The dump form draws on the same per-read
    /// scratch buffer a short name unpacks into, so the bytes are copied
    /// out before this returns rather than borrowed.
    #[inline]
    pub fn dump(self, mrb: &Mrb) -> Result<String, Error> {
        // SAFETY: `mrb` is alive.
        let ptr = unsafe { sys::mrb_sym_dump(mrb.as_ptr(), self.to_raw()) };
        // SAFETY: as `name`.
        let dump = unsafe { self.name_cstr(mrb, ptr) }?;
        Ok(dump.to_string_lossy().into_owned())
    }

    /// The C string a name read answered, or the `RangeError` for the NULL
    /// it answers when the interpreter holds no name for the id.
    ///
    /// # Safety
    ///
    /// A non-NULL `ptr` must be a valid C string for the returned borrow.
    unsafe fn name_cstr<'a>(
        self,
        mrb: &Mrb,
        ptr: *const core::ffi::c_char,
    ) -> Result<&'a core::ffi::CStr, Error> {
        if ptr.is_null() {
            return Err(unnamed(mrb));
        }
        // SAFETY: the caller's guarantee for a non-NULL `ptr`.
        Ok(unsafe { core::ffi::CStr::from_ptr(ptr) })
    }
}

/// The `RangeError` a name read surfaces for an id the interpreter holds no
/// name for — one past its symbol table, which only an id breaking the
/// interpreter pairing is (`sym2name_len`, `vendor/mruby/src/symbol.c`).
fn unnamed(mrb: &Mrb) -> Error {
    crate::error::core_error(mrb, c"RangeError", "symbol id out of range")
}

/// Typed handle on an mruby `Symbol`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Symbol::new` (intern a name), `Symbol::from` an `Id`,
/// the checked `FromValue` downcast (`Symbol::from_value`,
/// tag-discriminated), or `Symbol::from_value_unchecked`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Symbol(pub(crate) Value);

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

    /// Intern `name` and symbolize it. Mirrors magnus's `Symbol::new`,
    /// surfacing the intern's `Err` for a name too long to be a symbol,
    /// a limit CRuby does not have.
    #[inline]
    pub fn new<T: AsRef<str>>(mrb: &Mrb, name: T) -> Result<Self, Error> {
        mrb.intern(name.as_ref().as_bytes()).map(Self::from)
    }

    /// The symbol's name as an owned `String`, via `Id::name`, and with its
    /// `Err` for a name that is not UTF-8. Mirrors magnus's `Symbol::name`.
    #[inline]
    pub fn name(self, mrb: &Mrb) -> Result<String, Error> {
        Id::from(self).name(mrb)
    }

    /// The symbol's raw name bytes as an owned `Vec<u8>`, via
    /// `Id::name_bytes` — an embedded NUL preserved unescaped, where
    /// `name` returns the quoted dump form.
    #[inline]
    pub fn name_bytes(self, mrb: &Mrb) -> Result<Vec<u8>, Error> {
        Id::from(self).name_bytes(mrb)
    }

    /// The symbol's dump form as an owned `String`, via `Id::dump` — the
    /// bare name for a plain identifier, otherwise the quoted and escaped
    /// form (Ruby's `Symbol#inspect` without the leading colon).
    #[inline]
    pub fn dump(self, mrb: &Mrb) -> Result<String, Error> {
        Id::from(self).dump(mrb)
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
///
/// A variable or constant operation keys only a name its namespace
/// admits, which magnus's `IntoId` never checks: mruby keeps state its C
/// code reads back unchecked under names no Ruby program can write, where
/// CRuby keeps it out of every name table.
pub trait IntoId {
    /// Resolve this key to its `Id` against `mrb`, or the `Err` its
    /// intern surfaced.
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error>;

    /// Resolve this key in `space`, refusing a name it does not admit.
    /// `Sealed` keeps it from being overridden outside this crate.
    #[doc(hidden)]
    #[inline]
    fn into_id_in(self, mrb: &Mrb, space: Namespace, _: Sealed) -> Result<Id, Error>
    where
        Self: Sized,
    {
        let id = self.into_id(mrb)?;
        space.admit(mrb, id)
    }
}

impl IntoId for &core::ffi::CStr {
    /// Interns the bytes before the first NUL, so a name too long to be
    /// a symbol surfaces as `Err`.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        mrb.intern_cstr(self)
    }

    #[inline]
    fn into_id_in(self, mrb: &Mrb, space: Namespace, _: Sealed) -> Result<Id, Error> {
        space.intern(mrb, self.to_bytes())
    }
}

impl IntoId for &str {
    /// Interns all of the key's bytes, an embedded NUL included, so a
    /// name too long to be a symbol surfaces as `Err`.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        mrb.intern(self.as_bytes())
    }

    #[inline]
    fn into_id_in(self, mrb: &Mrb, space: Namespace, _: Sealed) -> Result<Id, Error> {
        space.intern(mrb, self.as_bytes())
    }
}

impl IntoId for String {
    /// Interns as the `&str` key does, for a name a caller owns.
    #[inline]
    fn into_id(self, mrb: &Mrb) -> Result<Id, Error> {
        self.as_str().into_id(mrb)
    }

    #[inline]
    fn into_id_in(self, mrb: &Mrb, space: Namespace, _: Sealed) -> Result<Id, Error> {
        space.intern(mrb, self.as_bytes())
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

crate::value::value_backed_repr!(Symbol);

crate::try_convert::tagged_conversions!(Symbol, sys::MRB_TT_SYMBOL => "Symbol");

// SAFETY: a symbol value is immediate, and mruby frees no symbol before the
// interpreter closes (`mrb_free_symtbl`, `vendor/mruby/src/symbol.c:625`),
// so every symbol carries the guarantee magnus's `StaticSymbol` marks owned.
unsafe impl crate::TryConvertOwned for Symbol {}

/// An `Id` boxes into the symbol value it names.
impl IntoValue for crate::Id {
    #[inline]
    fn into_value(self, _mrb: &Mrb) -> Value {
        Symbol::from(self).0
    }
}
