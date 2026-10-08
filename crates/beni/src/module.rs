//! The `Module` trait, the registration and constant surface shared by
//! classes and modules — beni's mirror of `magnus::module`.

use crate::class::{bound_class, private, protect_register};
use crate::{
    Class, Error, ExceptionClass, IntoId, IntoValue, MethodDef, Mrb, Object, RClass, RModule,
    ReprValue, TryConvert, Value,
};
use beni_sys as sys;

/// Registration and constant surface shared by classes and modules —
/// beni's mirror of `magnus::Module`. Every raising method runs inside
/// exception protection, so an mruby raise surfaces as
/// `Err(Error::Exception)` and never unwinds across FFI.
pub trait Module: Object + private::ClassLike {
    /// `mrb_define_class_under_id(mrb, self, name, superclass)` —
    /// define (or fetch) the nested class `self::name` inheriting from
    /// `superclass`. The name is a symbol-or-name key (`IntoId`). A
    /// name `self` already binds yields that ordinary class itself when
    /// `superclass` is its superclass, prepended modules and all, and a
    /// `TypeError` for anything else bound there.
    fn define_class<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        superclass: RClass,
    ) -> Result<RClass, Error> {
        let sym = name.into_id(mrb)?;
        if let Some(bound) = bound_class(mrb, self.raw(), sym, superclass) {
            return bound;
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` and `superclass` originate from the same VM;
            // `sym` was interned against the same VM.
            RClass::from_raw_unchecked(unsafe {
                sys::mrb_define_class_under_id(
                    mrb.as_ptr(),
                    self.raw(),
                    sym.to_raw(),
                    superclass.as_internal(),
                )
            })
        })
    }

    /// Define (or fetch) the nested exception class `self::name`
    /// descending from `superclass`, yielding it as an `ExceptionClass`.
    /// Mirrors magnus's `Module::define_error`; rejected as
    /// `Module::define_class` is.
    fn define_error<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        superclass: ExceptionClass,
    ) -> Result<ExceptionClass, Error> {
        // A class defined or fetched under an exception-class superclass
        // descends from it, so it is an exception class too.
        self.define_class(mrb, name, superclass.as_r_class())
            .map(ExceptionClass::from_descendant_unchecked)
    }

    /// `mrb_define_module_under_id(mrb, self, name)` — define (or
    /// fetch) the nested module `self::name`. The name is a
    /// symbol-or-name key (`IntoId`). mruby rejects a same-named
    /// constant that is not a module.
    fn define_module<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<RModule, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RModule::from_raw_unchecked(unsafe {
                sys::mrb_define_module_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_class_get_under_id(mrb, self, name)` — fetch the nested
    /// class `self::name`. The name is a symbol-or-name key
    /// (`IntoId`). mruby raises `NameError` when the constant is
    /// missing and `TypeError` when it is not a class (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    fn class_get<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<RClass, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RClass::from_raw_unchecked(unsafe {
                sys::mrb_class_get_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_module_get_under_id(mrb, self, name)` — fetch the nested
    /// module `self::name`. The name is a symbol-or-name key
    /// (`IntoId`). mruby raises `NameError` when the constant is
    /// missing and `TypeError` when it is not a module (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    fn module_get<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<RModule, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RModule::from_raw_unchecked(unsafe {
                sys::mrb_module_get_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_class_defined_under_id(mrb, self, name)` — TRUE when a
    /// class or module is defined under `self::name`. The name is a
    /// symbol-or-name key (`IntoId`), routed through the `_id` form
    /// like `class_get`. A total predicate: an undefined name reads
    /// `false` rather than raising, so it is the precondition test
    /// before a namespaced fetching lookup that would raise on a
    /// missing name. A name too long to intern can never be bound, so
    /// it reads `false` too.
    fn class_defined<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_id(mrb) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` originates from the same
        // VM; `sym` was interned against it. `mrb_class_defined_under_id`
        // is a constant-existence lookup that does not raise.
        unsafe { sys::mrb_class_defined_under_id(mrb.as_ptr(), self.raw(), sym.to_raw()) }
    }

    /// `mrb_define_method_id(mrb, self, name, func, aspec)` — register
    /// an instance method from a `method!`-wrapped Rust function. The
    /// name is a symbol-or-name key (`IntoId`). The aspec is derived
    /// from the wrapper's arity (`-1` = any arguments, `0..` = that
    /// many required positionals). mruby rejects registration on a
    /// frozen receiver.
    fn define_method<K: IntoId>(self, mrb: &Mrb, name: K, method: MethodDef) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` was produced by the same VM; `sym` was
            // interned against it; `raw` has the C ABI mruby expects.
            unsafe { sys::mrb_define_method_id(mrb.as_ptr(), self.raw(), sym, raw, aspec) };
        })
    }

    /// `mrb_define_private_method_id(mrb, self, name, func, aspec)` —
    /// like `define_method`, with private visibility: Ruby-level
    /// dispatch with an explicit receiver raises `NoMethodError`. The
    /// name is a symbol-or-name key (`IntoId`). The aspec derivation
    /// and rejection contract match `define_method`.
    fn define_private_method<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: as `define_method` — same signature, same
            // contract.
            unsafe { sys::mrb_define_private_method_id(mrb.as_ptr(), self.raw(), sym, raw, aspec) };
        })
    }

    /// `mrb_define_module_function_id(mrb, self, name, func, aspec)` —
    /// register a module function: one call defines both a private
    /// instance method, for a class that mixes the module in, and a
    /// singleton method on the module object, the way Ruby's
    /// `module_function` exposes `Math.sqrt`. The name is a
    /// symbol-or-name key (`IntoId`). The aspec derivation and
    /// rejection contract match `define_method`.
    fn define_module_function<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: as `define_method` — same signature, same
            // contract.
            unsafe {
                sys::mrb_define_module_function_id(mrb.as_ptr(), self.raw(), sym, raw, aspec)
            };
        })
    }

    /// `mrb_define_alias_id(mrb, self, new, old)` — bind `new` as a
    /// second name for the existing method `old` on this class or module,
    /// so a core method can be preserved before it is overridden. Both
    /// names are symbol-or-name keys (`IntoId`), each interned to its
    /// symbol before the `_id` alias call. Runs inside exception protection, so
    /// aliasing a method that does not exist surfaces as
    /// `Err(Error::Exception)` (mruby's `NameError`) rather than
    /// long-jumping — the same contract as the definition methods above.
    fn alias_method<N: IntoId, O: IntoId>(self, mrb: &Mrb, new: N, old: O) -> Result<(), Error> {
        let new = new.into_id(mrb)?.to_raw();
        let old = old.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `new` and `old`
            // were interned against it. The original-method lookup
            // raises NameError when `old` is absent — caught by
            // `protect`.
            unsafe { sys::mrb_define_alias_id(mrb.as_ptr(), self.raw(), new, old) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_undef_method_id(mrb, self, name)` — undefine a method on
    /// this class or module, Ruby's `Module#undef_method`: the name is
    /// marked as not defined on the handle even when an ancestor defines
    /// it. The name is a symbol-or-name key (`IntoId`); both forms
    /// resolve to the same interned id and route through the raising
    /// `_id` C function, so undefining a name absent from the handle and
    /// its ancestors surfaces as `Err(Error::Exception)` (mruby's
    /// `NameError`) rather than long-jumping — the same contract as the
    /// definition methods above.
    fn undef_method<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `sym` was interned
            // against it. `mrb_undef_method_id` raises NameError when
            // the method is absent — caught by `protect`.
            unsafe { sys::mrb_undef_method_id(mrb.as_ptr(), self.raw(), sym) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_remove_method(mrb, self, name)` — remove a method from this
    /// class or module, Ruby's `Module#remove_method`: the method's own
    /// definition is deleted from the handle, so the name reverts to any
    /// ancestor's method — distinct from `undef_method`, which masks
    /// ancestor lookups rather than stripping the definition. The name is a
    /// symbol-or-name key (`IntoId`). Removing a name not defined directly
    /// on the handle raises `NameError`; under exception protection it surfaces as
    /// `Err(Error::Exception)` rather than long-jumping — the same contract
    /// as the definition methods above.
    fn remove_method<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `sym` was interned
            // against it. `mrb_remove_method` raises NameError when the
            // method is not defined on the handle — caught by `protect`.
            unsafe { sys::mrb_remove_method(mrb.as_ptr(), self.raw(), sym) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_include_module(mrb, self, module)` — mix `module` into this
    /// class or module, Ruby's `include`. A frozen receiver raises
    /// `FrozenError` and a cyclic include raises `ArgumentError`; both
    /// surface as `Err` via exception protection.
    fn include_module(self, mrb: &Mrb, module: RModule) -> Result<(), Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // and `module` originate from the same VM. `mrb_include_module`
            // checks frozen state and rejects a cyclic include, raising
            // FrozenError or ArgumentError — caught by `protect`.
            unsafe { sys::mrb_include_module(mrb.as_ptr(), self.raw(), module.as_internal()) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_prepend_module(mrb, self, module)` — mix `module` into this
    /// class or module ahead of the receiver, Ruby's `prepend`, so the
    /// module's methods override the receiver's own. A frozen receiver
    /// raises `FrozenError` and a cyclic prepend raises `ArgumentError`;
    /// both surface as `Err` via exception protection.
    fn prepend_module(self, mrb: &Mrb, module: RModule) -> Result<(), Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // and `module` originate from the same VM. `mrb_prepend_module`
            // checks frozen state and rejects a cyclic prepend, raising
            // FrozenError or ArgumentError — caught by `protect`.
            unsafe { sys::mrb_prepend_module(mrb.as_ptr(), self.raw(), module.as_internal()) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_class_path(mrb, self)` — the handle's fully-qualified path,
    /// the namespace chain leading to it (`"Outer::Inner"` for a nested
    /// class, the bare name for a top-level one), or `None` when the
    /// handle is anonymous and has no place in any namespace. A total
    /// read that never raises, returning an owned `String` copied out of
    /// mruby's freshly built string. A class handle's `Class::name`
    /// answers a synthesized stand-in where this answers nothing.
    fn path(self, mrb: &Mrb) -> Option<String> {
        use crate::FromValue;
        // SAFETY: `mrb` is alive by the borrow; `self` originates
        // from the same VM by the single-VM contract. `mrb_class_path`
        // walks the namespace chain and never raises; it answers nil
        // for an anonymous handle and a String otherwise.
        let value =
            Value::from_raw_unchecked(unsafe { sys::mrb_class_path(mrb.as_ptr(), self.raw()) });
        if value.is_nil() {
            return None;
        }
        String::from_value(value)
    }

    /// `mrb_const_defined(mrb, self, sym)` — TRUE when constant `name`
    /// is defined on `self` or an ancestor. A name too long to be a
    /// symbol answers false.
    #[inline]
    fn const_defined<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_id(mrb).map(crate::Id::to_raw) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` is a class or module of the
        // same VM, the receiver `mrb_const_defined` reads unchecked.
        unsafe { sys::mrb_const_defined(mrb.as_ptr(), self.as_value().0, sym) }
    }

    /// `mrb_const_defined_at(mrb, self, sym)` — TRUE when constant
    /// `name` is defined directly on `self` alone, never one inherited
    /// from an ancestor; contrast `const_defined`, which walks the
    /// ancestry. A name too long to be a symbol answers false.
    #[inline]
    fn const_defined_at<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        name.into_id(mrb)
            .is_ok_and(|id| self.as_value().defines_const_at(mrb, id))
    }

    /// `mrb_const_get(mrb, self, sym)` — fetch constant `name` from
    /// `self` through `TryConvert` into `U`. Surfaces an `Err` when the
    /// name resolves to no constant, its `const_missing` hook raises, or
    /// the value does not convert.
    #[inline]
    fn const_get<K: IntoId, U: TryConvert>(self, mrb: &Mrb, name: K) -> Result<U, Error> {
        let value = self.as_value().fetch_const(mrb, name.into_id(mrb)?)?;
        U::try_convert(value, mrb)
    }

    /// `mrb_const_set(mrb, self, sym, val)` — assign constant `name` on
    /// `self` to `val`. Surfaces an `Err` when `self` is frozen or its
    /// `const_added` hook raises.
    #[inline]
    fn const_set<K: IntoId, U: IntoValue>(self, mrb: &Mrb, name: K, val: U) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        let val = val.into_value(mrb);
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // and `val` originate from the same VM. `mrb_const_set`
            // raises `FrozenError` on a frozen receiver and runs a
            // `const_added` hook that may raise — caught by `protect`.
            unsafe { sys::mrb_const_set(mrb.as_ptr(), self.as_value().0, sym, val.0) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_const_remove(mrb, self, sym)` — remove constant `name` from
    /// `self`, discarding its former value. An absent constant is a
    /// no-op; surfaces an `Err` when `self` is frozen.
    #[inline]
    fn const_remove<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_const_remove` raises
            // `FrozenError` on a frozen receiver — caught by `protect`.
            unsafe { sys::mrb_const_remove(mrb.as_ptr(), self.as_value().0, sym) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_cv_get(mrb, self, sym)` — read class variable `name` from
    /// `self`, walking the ancestry, through `TryConvert` into `U`.
    /// Surfaces an `Err` when the name resolves to no class variable or
    /// the value does not convert.
    #[inline]
    fn cvar_get<K: IntoId, U: TryConvert>(self, mrb: &Mrb, name: K) -> Result<U, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        let value = mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_cv_get` raises
            // `NameError` for an undefined class variable — caught by
            // `protect`.
            Value(unsafe { sys::mrb_cv_get(mrb.as_ptr(), self.as_value().0, sym) })
        })?;
        U::try_convert(value, mrb)
    }

    /// `mrb_cv_set(mrb, self, sym, val)` — assign class variable `name`
    /// on `self` to `val`. Surfaces an `Err` when `self` is frozen;
    /// `mrb_mod_cv_set`, the raw-`RClass*` form, stays in `sys`.
    #[inline]
    fn cvar_set<K: IntoId, U: IntoValue>(self, mrb: &Mrb, name: K, val: U) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        let val = val.into_value(mrb);
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // and `val` originate from the same VM. `mrb_cv_set` raises
            // `FrozenError` on a frozen receiver — caught by `protect`.
            unsafe { sys::mrb_cv_set(mrb.as_ptr(), self.as_value().0, sym, val.0) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_cv_defined(mrb, self, sym)` — TRUE when class variable
    /// `name` is defined on `self` or an ancestor. A name too long to be
    /// a symbol answers false; `mrb_mod_cv_defined`, the raw-`RClass*`
    /// form, stays in `sys`.
    #[inline]
    fn cvar_defined<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_id(mrb).map(crate::Id::to_raw) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` is a class or module of the
        // same VM, the receiver `mrb_cv_defined` reads unchecked.
        unsafe { sys::mrb_cv_defined(mrb.as_ptr(), self.as_value().0, sym) }
    }
}

impl Module for RClass {}
impl Module for RModule {}
impl Module for ExceptionClass {}
