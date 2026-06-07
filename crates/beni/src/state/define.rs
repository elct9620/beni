//! Top-level module / class registration and global-state access
//! on `Mrb`.
//!
//! Inherent methods that work against the Object root or the global
//! variable table:
//!
//!   * `mrb_define_module` / `mrb_define_class` — register a new
//!     module or class at top level.
//!   * `mrb_class_get` — look one up by name.
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

use crate::{Error, Mrb, RClass, RModule, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_define_module(mrb, name)` — return the module named
    /// `name`, defining it at top level if not already present.
    /// mruby rejects a same-named constant that is not a module.
    #[inline]
    pub fn define_module(&self, name: &core::ffi::CStr) -> Result<RModule, Error> {
        #[cfg(mruby_linked)]
        {
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `name` is NUL-terminated.
                unsafe { sys::mrb_define_module(mrb.as_ptr(), name.as_ptr()) }
            })
            .map(RModule::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = name;
            crate::not_linked()
        }
    }

    /// `mrb_define_class(mrb, name, super_)` — define a top-level
    /// class named `name` inheriting from `super_`. mruby rejects a
    /// superclass mismatch with an existing definition, or a
    /// same-named constant that is not a class.
    #[inline]
    pub fn define_class(&self, name: &core::ffi::CStr, super_: RClass) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`; `super_` was produced by
                // the same VM.
                unsafe { sys::mrb_define_class(mrb.as_ptr(), name.as_ptr(), super_.as_raw()) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (name, super_);
            crate::not_linked()
        }
    }

    /// `mrb_class_get(mrb, name)` — fetch the top-level class named
    /// `name`. mruby raises `NameError` when the constant is missing
    /// and `TypeError` when it is not a class (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    #[inline]
    pub fn class_get(&self, name: &core::ffi::CStr) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            crate::class::protect_class_ptr(self, |mrb| {
                // SAFETY: as `define_module`.
                unsafe { sys::mrb_class_get(mrb.as_ptr(), name.as_ptr()) }
            })
            .map(RClass::from_raw)
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
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{FromValue, Mrb, Value};

    #[test]
    fn gv_get_reads_nil_for_unset_global() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let sym = mrb.intern_cstr(c"$beni_gv_unset");

        assert!(mrb.gv_get(sym).is_nil());
    }

    #[test]
    fn gv_get_observes_reassignment() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let sym = mrb.intern_cstr(c"$beni_gv");

        // Globals are read at call time: each assignment must be
        // visible to the next read, the contract redirection-style
        // consumers (`$stdout = $stderr`) rely on.
        mrb.gv_set(sym, Value::from_int(&mrb, 1));
        assert_eq!(i32::from_value(mrb.gv_get(sym)), Some(1));

        mrb.gv_set(sym, Value::from_int(&mrb, 2));
        assert_eq!(i32::from_value(mrb.gv_get(sym)), Some(2));
    }
}
