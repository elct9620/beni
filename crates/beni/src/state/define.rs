//! Top-level module / class registration and global-state access
//! on `Mrb`.
//!
//! Inherent methods that work against the Object root or the global
//! variable table:
//!
//!   * `mrb_define_module` / `mrb_define_class` — register a new
//!     module or class at top level.
//!   * `mrb_class_new` / `mrb_module_new` — create an anonymous class
//!     or module, bound to no constant name.
//!   * `mrb_class_get` / `mrb_module_get` — look one up by name.
//!   * `mrb_class_defined` — test whether one is defined by name.
//!   * `mrb_exc_get_id` — look up a built-in exception class by name.
//!   * `mrb_define_global_const` — bind a top-level constant.
//!   * `mrb_gv_set` / `mrb_gv_get` — assign or read a Ruby `$global`.
//!
//! Class and module definitions and lookups run inside
//! exception protection so an mruby raise surfaces as
//! `Err(Error::Exception)` — the same contract as the `Module`
//! trait, whose nested-namespace counterparts (`define_class` /
//! `define_module` / `class_get` on a handle) live on
//! `crate::RClass` / `crate::RModule`. Global variable access is a
//! plain table operation that cannot raise.

use crate::{sys::AsRawValue, Error, ExceptionClass, IntoId, Mrb, RClass, RModule, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_define_module_id(mrb, name)` — return the module named
    /// `name`, defining it at top level if not already present. The
    /// name is a symbol-or-name key (`IntoId`). mruby rejects a
    /// same-named constant that is not a module.
    #[inline]
    pub fn define_module<K: IntoId>(&self, name: K) -> Result<RModule, Error> {
        let sym = name.into_id(self)?.to_raw();
        self.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `sym` was interned against the same VM.
            RModule::from_raw_unchecked(unsafe { sys::mrb_define_module_id(mrb.as_ptr(), sym) })
        })
    }

    /// `mrb_define_class_id(mrb, name, super_)` — define (or fetch) a
    /// top-level class named `name` inheriting from `super_`. The name is
    /// a symbol-or-name key (`IntoId`). A name already bound at the top
    /// level yields that ordinary class itself when `super_` is its
    /// superclass, prepended modules and all, and a `TypeError` for
    /// anything else bound there.
    #[inline]
    pub fn define_class<K: IntoId>(&self, name: K, super_: RClass) -> Result<RClass, Error> {
        let sym = name.into_id(self)?;
        if let Some(bound) =
            crate::class::bound_class(self, self.object_class().as_internal(), sym, super_)
        {
            return bound;
        }
        self.protect(|mrb| {
            // SAFETY: as `define_module`; `super_` was produced by
            // the same VM.
            RClass::from_raw_unchecked(unsafe {
                sys::mrb_define_class_id(mrb.as_ptr(), sym.to_raw(), super_.as_internal())
            })
        })
    }

    /// Define (or fetch) the top-level exception class named `name`
    /// descending from `superclass`, yielding it as an `ExceptionClass`.
    /// Mirrors magnus's `define_error`. The name is a symbol-or-name key
    /// (`IntoId`), and a name already bound resolves as
    /// `Mrb::define_class` resolves it.
    #[inline]
    pub fn define_error<K: IntoId>(
        &self,
        name: K,
        superclass: ExceptionClass,
    ) -> Result<ExceptionClass, Error> {
        // A class defined or fetched under an exception-class superclass
        // descends from it, so it is an exception class too.
        self.define_class(name, superclass.as_r_class())
            .map(|class| ExceptionClass::from_raw_unchecked(class.as_internal()))
    }

    /// `mrb_class_new(mrb, super_)` — create an anonymous class
    /// inheriting from `super_`, bound to no constant. The class gains a
    /// name only when later bound to a constant. mruby rejects a
    /// superclass that is not an ordinary class — a singleton class or
    /// `Class` itself — so the creation is fallible by contract.
    #[inline]
    pub fn class_new(&self, super_: RClass) -> Result<RClass, Error> {
        self.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `super_` was produced by the same VM.
            RClass::from_raw_unchecked(unsafe {
                sys::mrb_class_new(mrb.as_ptr(), super_.as_internal())
            })
        })
    }

    /// `mrb_module_new(mrb)` — create an anonymous module, bound to no
    /// constant. The module gains a name only when later bound to a
    /// constant. Allocation alone never raises.
    #[inline]
    pub fn module_new(&self) -> RModule {
        // SAFETY: `self` is alive by the borrow; the allocation
        // happens against the same VM.
        RModule::from_raw_unchecked(unsafe { sys::mrb_module_new(self.as_ptr()) })
    }

    /// `mrb_class_get_id(mrb, name)` — fetch the top-level class named
    /// `name`. The name is a symbol-or-name key (`IntoId`). mruby
    /// raises `NameError` when the constant is missing and `TypeError`
    /// when it is not a class (vendored `src/class.c` documents both),
    /// so the lookup is fallible by contract.
    #[inline]
    pub fn class_get<K: IntoId>(&self, name: K) -> Result<RClass, Error> {
        let sym = name.into_id(self)?.to_raw();
        self.protect(|mrb| {
            // SAFETY: as `define_module`.
            RClass::from_raw_unchecked(unsafe { sys::mrb_class_get_id(mrb.as_ptr(), sym) })
        })
    }

    /// `mrb_class_defined_id(mrb, name)` — TRUE when a class or module
    /// is defined under `name` at top level. The name is a
    /// symbol-or-name key (`IntoId`), routed through the `_id` form
    /// like `class_get`. A total predicate: an undefined name reads
    /// `false` rather than raising, so it is the precondition test
    /// before a fetching lookup that would raise on a missing name. A
    /// name too long to intern can never be bound, so it reads `false`
    /// too.
    #[inline]
    pub fn class_defined<K: IntoId>(&self, name: K) -> bool {
        let Ok(sym) = name.into_id(self) else {
            return false;
        };
        // SAFETY: `self` is alive; `sym` was interned against the
        // same VM. `mrb_class_defined_id` is a constant-existence
        // lookup that does not raise.
        unsafe { sys::mrb_class_defined_id(self.as_ptr(), sym.to_raw()) }
    }

    /// `mrb_exc_get_id(mrb, name)` — fetch the built-in exception
    /// class named `name` as an `ExceptionClass`. The name is a
    /// symbol-or-name key (`IntoId`). mruby raises when the constant is
    /// missing, is not a class, or is a class that is not `Exception` or
    /// a class descending from it (vendored `src/class.c`), so the lookup
    /// is fallible by contract. This is the typed path to a built-in
    /// exception class — `RuntimeError`, `ArgumentError`, `TypeError` —
    /// for raising from registered code.
    #[inline]
    pub fn exc_get<K: IntoId>(&self, name: K) -> Result<ExceptionClass, Error> {
        let sym = name.into_id(self)?.to_raw();
        self.protect(|mrb| {
            // SAFETY: as `define_module`.
            let class = unsafe { sys::mrb_exc_get_id(mrb.as_ptr(), sym) };
            // `mrb_exc_get_id` returns only a class whose ancestry reaches
            // `Exception`, which is what makes it an exception class.
            ExceptionClass::from_raw_unchecked(class)
        })
    }

    /// `mrb_module_get_id(mrb, name)` — fetch the top-level module
    /// named `name`. The name is a symbol-or-name key (`IntoId`).
    /// mruby raises `NameError` when the constant is missing and
    /// `TypeError` when it is not a module (vendored `src/class.c`
    /// documents both), so the lookup is fallible by contract.
    #[inline]
    pub fn module_get<K: IntoId>(&self, name: K) -> Result<RModule, Error> {
        let sym = name.into_id(self)?.to_raw();
        self.protect(|mrb| {
            // SAFETY: as `define_module`.
            RModule::from_raw_unchecked(unsafe { sys::mrb_module_get_id(mrb.as_ptr(), sym) })
        })
    }

    /// `mrb_define_global_const(mrb, name, val)` — bind a top-level
    /// constant. Reachable as `name` and as `Object::name`. Runs inside
    /// exception protection, so a frozen `Object` surfaces as
    /// `Err(Error::Exception)` rather than long-jumping, as
    /// `Module::define_const` does for any other receiver.
    #[inline]
    pub fn define_global_const(&self, name: &core::ffi::CStr, val: Value) -> Result<(), Error> {
        self.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `name` is
            // NUL-terminated; `val` originates from the same VM.
            unsafe { sys::mrb_define_global_const(mrb.as_ptr(), name.as_ptr(), val.as_raw()) };
            Value::nil()
        })
        .map(|_| ())
    }

    /// `mrb_gv_set(mrb, sym, val)` — assign the global variable named
    /// by a symbol-or-name key (`IntoId`). The assignment itself never
    /// fails; the `Err` it carries is the key's own.
    #[inline]
    pub fn gv_set<K: IntoId>(&self, name: K, val: Value) -> Result<(), Error> {
        let sym = name.into_id(self)?.to_raw();
        // SAFETY: `self` is alive; `val` originates from the same VM.
        unsafe { sys::mrb_gv_set(self.as_ptr(), sym, val.as_raw()) };
        Ok(())
    }

    /// `mrb_gv_get(mrb, sym)` — read the global variable named by a
    /// symbol-or-name key (`IntoId`); an unset global reads as nil, as
    /// does a key too long to name a symbol. The read happens at call
    /// time, so a reassigned global yields its current value.
    #[inline]
    pub fn gv_get<K: IntoId>(&self, name: K) -> Value {
        let Ok(sym) = name.into_id(self).map(crate::Id::to_raw) else {
            return Value::nil();
        };
        // SAFETY: `self` is alive; `sym` was interned against it.
        self.hold(Value::from_raw_unchecked(unsafe {
            sys::mrb_gv_get(self.as_ptr(), sym)
        }))
    }

    /// `mrb_gv_remove(mrb, sym)` — remove the global variable named by a
    /// symbol-or-name key (`IntoId`). Removing an unset global is a
    /// no-op, as is a key too long to name a symbol; neither case
    /// raises. The global reads as nil afterwards, the same as one never
    /// set.
    #[inline]
    pub fn gv_remove<K: IntoId>(&self, name: K) {
        let Ok(sym) = name.into_id(self).map(crate::Id::to_raw) else {
            return;
        };
        // SAFETY: `self` is alive; `sym` was interned against it.
        // `mrb_gv_remove` deletes the entry and does not raise.
        unsafe { sys::mrb_gv_remove(self.as_ptr(), sym) };
    }
}
