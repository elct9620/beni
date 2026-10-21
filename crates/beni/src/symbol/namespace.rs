//! The namespaces a name key reaches, and the names each admits.
//!
//! mruby keeps state its C code reads back unchecked under names no Ruby
//! program can write: a singleton class's `__attached__`, which class,
//! constant, and class-variable tables share and `mrb_mod_cv_get` reads
//! as a class pointer with no type check (`vendor/mruby/src/variable.c`),
//! a method object's `_proc` and a binding's `proc` (`mrbgems/mruby-method`,
//! `mrbgems/mruby-binding`), and the `_gc_root_` global (`src/gc.c`).
//! CRuby keeps such state in struct fields and nameless ids, so magnus's
//! `IntoId` checks nothing; here a key reaches a variable or constant
//! only under a name its namespace admits. The rules are those mruby's
//! reflection applies — `mrb_iv_name_sym_p` (`src/variable.c`),
//! `cv_name_p` (`mrbgems/mruby-metaprog`), and `mrb_const_name_p`
//! (`src/class.c`) — and a global name without `$` keys the `$`-prefixed
//! one, as CRuby's `rb_gv_get` does. A data carrier's instance variables
//! are the exception: mruby's data type has no mark hook, so its hidden
//! ones keep what its payload holds, where magnus's payload marks.

use super::IntoId;
use crate::{Error, Id, Mrb};
use beni_sys as sys;
use core::ffi::CStr;

/// A namespace a name key reaches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Ivar,
    Cvar,
    Const,
    Gvar,
}

/// What only this crate can produce, so `IntoId::into_id_in` has no
/// override outside it and every key passes its namespace's check.
pub struct Sealed(pub(super) ());

impl Namespace {
    /// The id the name `bytes` spells keys in this namespace, interned.
    /// A refused name is interned too, so the `NameError` can name it,
    /// and a name too long to intern surfaces that `Err` first.
    pub(crate) fn intern(self, mrb: &Mrb, bytes: &[u8]) -> Result<Id, Error> {
        if self == Self::Gvar {
            return match bytes.first() {
                Some(b'$') => mrb.intern(bytes),
                _ => mrb.intern(&[&b"$"[..], bytes].concat()),
            };
        }
        let id = mrb.intern(bytes)?;
        match self.refusal(bytes) {
            None => Ok(id),
            Some(message) => Err(refuse(mrb, id, message)),
        }
    }

    /// `id` checked for this namespace; for a global, the id its
    /// `$`-prefixed name interns to when it lacks one.
    pub(crate) fn admit(self, mrb: &Mrb, id: Id) -> Result<Id, Error> {
        let name = name_of(mrb, id);
        if self == Self::Gvar {
            return match name.first() {
                Some(b'$') => Ok(id),
                _ => mrb.intern(&[&b"$"[..], name].concat()),
            };
        }
        match self.refusal(name) {
            None => Ok(id),
            Some(message) => Err(refuse(mrb, id, message)),
        }
    }

    /// Whether this namespace admits the name `id` spells.
    pub(crate) fn admits(self, mrb: &Mrb, id: Id) -> bool {
        self.refusal(name_of(mrb, id)).is_none()
    }

    /// mruby's `NameError` message for `name` when this namespace does
    /// not admit it, or nothing when it does.
    fn refusal(self, name: &[u8]) -> Option<&'static CStr> {
        let (tail, message) = match (self, name) {
            (Self::Ivar, [b'@', tail @ ..]) => (tail, IVAR_REFUSAL),
            (Self::Cvar, [b'@', b'@', tail @ ..]) => (tail, CVAR_REFUSAL),
            (Self::Const, [first, tail @ ..]) if first.is_ascii_uppercase() => {
                return (!tail.iter().all(|&c| identifier(c))).then_some(CONST_REFUSAL);
            }
            (Self::Ivar, _) => return Some(IVAR_REFUSAL),
            (Self::Cvar, _) => return Some(CVAR_REFUSAL),
            (Self::Const, _) => return Some(CONST_REFUSAL),
            (Self::Gvar, _) => return None,
        };
        let admitted = tail.first().is_some_and(|c| !c.is_ascii_digit())
            && tail.iter().all(|&c| identifier(c));
        (!admitted).then_some(message)
    }
}

const IVAR_REFUSAL: &CStr = c"'%n' is not allowed as an instance variable name";
const CVAR_REFUSAL: &CStr = c"'%n' is not allowed as a class variable name";
const CONST_REFUSAL: &CStr = c"wrong constant name %n";

/// The name `id` spells.
fn name_of(mrb: &Mrb, id: Id) -> &[u8] {
    let mut len = 0;
    // SAFETY: `mrb` is live and `id` was interned against it.
    let ptr = unsafe { sys::mrb_sym_name_len(mrb.as_ptr(), id.to_raw(), &mut len) };
    if ptr.is_null() {
        return &[];
    }
    // SAFETY: mruby answers `len` bytes at `ptr`. A short name unpacks
    // into the state's scratch buffer, which no caller reads another
    // symbol into before it is done with these bytes.
    unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) }
}

/// mruby's `identchar`: an ASCII letter or digit, `_`, or a byte outside
/// ASCII.
fn identifier(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || !c.is_ascii()
}

/// The `NameError` mruby raises for `id` under `message`, naming it.
fn refuse(mrb: &Mrb, id: Id, message: &CStr) -> Error {
    let raised = mrb.protect_unit(|mrb| {
        // SAFETY: `mrb` is live inside the protect frame, `message` holds
        // one `%n` and `id` is the symbol it formats; `mrb_name_error`
        // raises, which the frame catches.
        unsafe { sys::mrb_name_error(mrb.as_ptr(), id.to_raw(), message.as_ptr(), id.to_raw()) }
    });
    match raised {
        Err(error) => error,
        Ok(()) => unreachable!("mrb_name_error always raises"),
    }
}

/// The id a key checked once for one namespace carries, keying that
/// namespace with no check repeating and any other as a plain `Id`.
macro_rules! checked_key {
    ($(#[$doc:meta])* $key:ident, $space:ident) => {
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $key(Id);

        impl From<$key> for Id {
            #[inline]
            fn from(key: $key) -> Self {
                key.0
            }
        }

        impl IntoId for $key {
            #[inline]
            fn into_id(self, _mrb: &Mrb) -> Result<Id, Error> {
                Ok(self.0)
            }

            #[inline]
            fn into_id_in(self, mrb: &Mrb, space: Namespace, _: Sealed) -> Result<Id, Error> {
                if space == Namespace::$space {
                    Ok(self.0)
                } else {
                    space.admit(mrb, self.0)
                }
            }
        }
    };
}

checked_key!(
    /// An instance-variable name checked once, so the instance-variable
    /// operations it keys repeat no check.
    IvarId,
    Ivar
);
checked_key!(
    /// A class-variable name checked once, so the class-variable
    /// operations it keys repeat no check.
    CvarId,
    Cvar
);
checked_key!(
    /// A constant name checked once, so the constant operations it keys
    /// repeat no check.
    ConstId,
    Const
);
checked_key!(
    /// A global-variable name resolved once to its `$`-prefixed id, so
    /// the global-variable operations it keys repeat no lookup.
    GvarId,
    Gvar
);

impl IvarId {
    /// Check `name` as an instance-variable name, surfacing the
    /// `NameError` an instance-variable operation would.
    #[inline]
    pub fn new<K: IntoId>(mrb: &Mrb, name: K) -> Result<Self, Error> {
        name.into_id_in(mrb, Namespace::Ivar, Sealed(())).map(Self)
    }
}

impl CvarId {
    /// Check `name` as a class-variable name, surfacing the `NameError`
    /// a class-variable operation would.
    #[inline]
    pub fn new<K: IntoId>(mrb: &Mrb, name: K) -> Result<Self, Error> {
        name.into_id_in(mrb, Namespace::Cvar, Sealed(())).map(Self)
    }
}

impl ConstId {
    /// Check `name` as a constant name, surfacing the `NameError` a
    /// constant operation would.
    #[inline]
    pub fn new<K: IntoId>(mrb: &Mrb, name: K) -> Result<Self, Error> {
        name.into_id_in(mrb, Namespace::Const, Sealed(())).map(Self)
    }
}

impl GvarId {
    /// Resolve `name` to the global it keys, `$` prefixed when it lacks
    /// one.
    #[inline]
    pub fn new<K: IntoId>(mrb: &Mrb, name: K) -> Result<Self, Error> {
        name.into_id_in(mrb, Namespace::Gvar, Sealed(())).map(Self)
    }
}

/// The id `name` keys in `space`, for the crate's name-keyed operations.
#[inline]
pub(crate) fn key<K: IntoId>(mrb: &Mrb, space: Namespace, name: K) -> Result<Id, Error> {
    name.into_id_in(mrb, space, Sealed(()))
}
