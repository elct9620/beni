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
//! `Mrb::protect` so an mruby raise surfaces as
//! `Err(Error::Exception)` — the same contract as the `Module`
//! trait, whose nested-namespace counterparts (`define_class` /
//! `define_module` / `class_get` on a handle) live on
//! `crate::RClass` / `crate::RModule`. Global variable access is a
//! plain table operation that cannot raise.

use crate::{Error, IntoSym, Mrb, RClass, RModule, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_define_module_id(mrb, name)` — return the module named
    /// `name`, defining it at top level if not already present. The
    /// name is a symbol-or-name key (`IntoSym`). mruby rejects a
    /// same-named constant that is not a module.
    #[inline]
    pub fn define_module<K: IntoSym>(&self, name: K) -> Result<RModule, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `sym` was interned against the same VM.
                unsafe { sys::mrb_define_module_id(mrb.as_ptr(), sym) }
            })
            .map(RModule::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_define_class_id(mrb, name, super_)` — define a top-level
    /// class named `name` inheriting from `super_`. The name is a
    /// symbol-or-name key (`IntoSym`). mruby rejects a superclass
    /// mismatch with an existing definition, or a same-named constant
    /// that is not a class.
    #[inline]
    pub fn define_class<K: IntoSym>(&self, name: K, super_: RClass) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`; `super_` was produced by
                // the same VM.
                unsafe { sys::mrb_define_class_id(mrb.as_ptr(), sym, super_.as_raw()) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (name, super_);
            crate::not_linked()
        }
    }

    /// `mrb_class_new(mrb, super_)` — create an anonymous class
    /// inheriting from `super_`, bound to no constant. The class gains a
    /// name only when later bound to a constant. mruby rejects a
    /// superclass that is not an ordinary class — a singleton class or
    /// `Class` itself — so the creation is fallible by contract.
    #[inline]
    pub fn class_new(&self, super_: RClass) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `super_` was produced by the same VM.
                unsafe { sys::mrb_class_new(mrb.as_ptr(), super_.as_raw()) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = super_;
            crate::not_linked()
        }
    }

    /// `mrb_module_new(mrb)` — create an anonymous module, bound to no
    /// constant. The module gains a name only when later bound to a
    /// constant. Allocation alone never raises.
    #[inline]
    pub fn module_new(&self) -> RModule {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the borrow; the allocation
            // happens against the same VM.
            RModule::from_raw(unsafe { sys::mrb_module_new(self.as_ptr()) })
        }
        #[cfg(not(mruby_linked))]
        {
            crate::not_linked()
        }
    }

    /// `mrb_class_get_id(mrb, name)` — fetch the top-level class named
    /// `name`. The name is a symbol-or-name key (`IntoSym`). mruby
    /// raises `NameError` when the constant is missing and `TypeError`
    /// when it is not a class (vendored `src/class.c` documents both),
    /// so the lookup is fallible by contract.
    #[inline]
    pub fn class_get<K: IntoSym>(&self, name: K) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`.
                unsafe { sys::mrb_class_get_id(mrb.as_ptr(), sym) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_class_defined_id(mrb, name)` — TRUE when a class or module
    /// is defined under `name` at top level. The name is a
    /// symbol-or-name key (`IntoSym`), routed through the `_id` form
    /// like `class_get`. A total predicate: an undefined name reads
    /// `false` rather than raising, so it is the precondition test
    /// before a fetching lookup that would raise on a missing name.
    #[inline]
    pub fn class_defined<K: IntoSym>(&self, name: K) -> bool {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            // SAFETY: `self` is alive; `sym` was interned against the
            // same VM. `mrb_class_defined_id` is a constant-existence
            // lookup that does not raise.
            unsafe { sys::mrb_class_defined_id(self.as_ptr(), sym) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_exc_get_id(mrb, name)` — fetch the built-in exception
    /// class named `name`, guaranteed to descend from `Exception`. The
    /// name is a symbol-or-name key (`IntoSym`). mruby raises when the
    /// constant is missing, is not a class, or is a class that is not
    /// an `Exception` subclass (vendored `src/class.c`), so the lookup
    /// is fallible by contract. This is the typed path to a built-in
    /// exception class — `RuntimeError`, `ArgumentError`, `TypeError` —
    /// for raising from registered code.
    #[inline]
    pub fn exc_get<K: IntoSym>(&self, name: K) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`.
                unsafe { sys::mrb_exc_get_id(mrb.as_ptr(), sym) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_module_get_id(mrb, name)` — fetch the top-level module
    /// named `name`. The name is a symbol-or-name key (`IntoSym`).
    /// mruby raises `NameError` when the constant is missing and
    /// `TypeError` when it is not a module (vendored `src/class.c`
    /// documents both), so the lookup is fallible by contract.
    #[inline]
    pub fn module_get<K: IntoSym>(&self, name: K) -> Result<RModule, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(self);
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`.
                unsafe { sys::mrb_module_get_id(mrb.as_ptr(), sym) }
            })
            .map(RModule::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_define_global_const(mrb, name, val)` — bind a top-level
    /// constant. Reachable as `name` and as `Object::name`.
    #[inline]
    pub fn define_global_const(&self, name: &core::ffi::CStr, val: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `name` is NUL-terminated; `val`
            // originates from the same VM.
            unsafe { sys::mrb_define_global_const(self.as_ptr(), name.as_ptr(), val.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (name, val);
            crate::not_linked()
        }
    }

    /// `mrb_gv_set(mrb, sym, val)` — assign a global variable.
    #[inline]
    pub fn gv_set(&self, sym: sys::mrb_sym, val: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `val` originates from the same VM.
            unsafe { sys::mrb_gv_set(self.as_ptr(), sym, val.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (sym, val);
            crate::not_linked()
        }
    }

    /// `mrb_gv_get(mrb, sym)` — read a global variable; an unset
    /// global reads as nil. The read happens at call time, so a
    /// reassigned global yields its current value.
    #[inline]
    pub fn gv_get(&self, sym: sys::mrb_sym) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `sym` was interned against the
            // same VM (caller contract).
            Value::from_raw(unsafe { sys::mrb_gv_get(self.as_ptr(), sym) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }

    /// `mrb_gv_remove(mrb, sym)` — remove a global variable. Removing
    /// an unset global is a no-op; neither case raises. The global
    /// reads as nil afterwards, the same as one never set.
    #[inline]
    pub fn gv_remove(&self, sym: sys::mrb_sym) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `sym` was interned against the
            // same VM (caller contract). `mrb_gv_remove` deletes the
            // entry and does not raise.
            unsafe { sys::mrb_gv_remove(self.as_ptr(), sym) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = sym;
            crate::not_linked()
        }
    }
}
