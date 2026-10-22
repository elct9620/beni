//! The core classes and modules mruby keeps on its state, read on `Mrb`
//! under magnus's accessor names.

use crate::{Mrb, RClass, RModule};
use beni_sys as sys;

impl Mrb {
    /// Ruby's `Object` class. Mirrors magnus's `Ruby::class_object`.
    #[inline]
    pub fn class_object(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.object_class))
    }

    /// Ruby's `Class` class. Mirrors magnus's `Ruby::class_class`.
    #[inline]
    pub fn class_class(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.class_class))
    }

    /// Ruby's `Module` class. Mirrors magnus's `Ruby::class_module`.
    #[inline]
    pub fn class_module(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.module_class))
    }

    /// Ruby's `Proc` class. Mirrors magnus's `Ruby::class_proc`.
    #[inline]
    pub fn class_proc(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.proc_class))
    }

    /// Ruby's `String` class. Mirrors magnus's `Ruby::class_string`.
    #[inline]
    pub fn class_string(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.string_class))
    }

    /// Ruby's `Symbol` class. Mirrors magnus's `Ruby::class_symbol`.
    #[inline]
    pub fn class_symbol(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.symbol_class))
    }

    /// Ruby's `Array` class. Mirrors magnus's `Ruby::class_array`.
    #[inline]
    pub fn class_array(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.array_class))
    }

    /// Ruby's `Hash` class. Mirrors magnus's `Ruby::class_hash`.
    #[inline]
    pub fn class_hash(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.hash_class))
    }

    /// Ruby's `Range` class. Mirrors magnus's `Ruby::class_range`.
    #[inline]
    pub fn class_range(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.range_class))
    }

    /// Ruby's `Integer` class. Mirrors magnus's `Ruby::class_integer`.
    #[inline]
    pub fn class_integer(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.integer_class))
    }

    /// Ruby's `Float` class. Mirrors magnus's `Ruby::class_float`.
    #[inline]
    pub fn class_float(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.float_class))
    }

    /// Ruby's `TrueClass` class. Mirrors magnus's `Ruby::class_true_class`.
    #[inline]
    pub fn class_true_class(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.true_class))
    }

    /// Ruby's `FalseClass` class. Mirrors magnus's `Ruby::class_false_class`.
    #[inline]
    pub fn class_false_class(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.false_class))
    }

    /// Ruby's `NilClass` class. Mirrors magnus's `Ruby::class_nil_class`.
    #[inline]
    pub fn class_nil_class(&self) -> RClass {
        RClass::from_raw_unchecked(self.state_field(|state| state.nil_class))
    }

    /// Ruby's `Kernel` module. Mirrors magnus's `Ruby::module_kernel`.
    #[inline]
    pub fn module_kernel(&self) -> RModule {
        RModule::from_raw_unchecked(self.state_field(|state| state.kernel_module))
    }

    /// One core class pointer `mrb_open` filled in and nothing replaces.
    fn state_field(
        &self,
        field: impl FnOnce(&sys::mrb_state) -> *mut sys::RClass,
    ) -> *mut sys::RClass {
        // SAFETY: `self` is alive by the `&self` borrow, and the read
        // leaves the state unchanged.
        field(unsafe { &*self.as_ptr() })
    }
}
