//! Typed `RClass` / `RModule` / `ExceptionClass` handles and the
//! `Module` / `Object` registration traits — beni's mirror of
//! `magnus::RClass` / `magnus::RModule` / `magnus::ExceptionClass` with
//! `magnus::Module` / `magnus::Object`.
//!
//! ## Why newtypes
//!
//! Same rationale as `Value`: mruby names a class by a raw
//! `*mut RClass`, easy to confuse with other opaque pointers and
//! impossible to attach inherent methods to from a sibling crate. A
//! consumer reaches that pointer only through the class value, as
//! magnus leaves the class struct. mruby represents classes and modules with the same
//! C `struct RClass`; the Rust newtypes keep "this handle is a class" /
//! "this handle is a module" / "this class allocates exceptions"
//! distinct at the type level while sharing the registration surface
//! through the traits.
//!
//! ## ABI guarantee
//!
//! Every handle is `#[repr(transparent)]` over `*mut RClass`, so
//! each is pointer-sized and shares the C ABI on every target — a
//! struct field of any of them round-trips into mruby's own
//! `RClass *` slot without conversion.
//!
//! ## Error contract
//!
//! Every definition, registration, lookup, and instance construction
//! runs inside exception protection, so an mruby raise (superclass mismatch,
//! frozen receiver, missing constant, a raising `initialize`, …)
//! surfaces as `Err(Error::Exception)` instead of long-jumping across
//! Rust frames.

use crate::{
    sys::AsRawValue, Error, FromValue, IntoId, IntoValue, MethodDef, Mrb, RString, ReprValue,
    TryConvert, Value,
};
use beni_sys as sys;

/// Typed handle on an mruby class. `#[repr(transparent)]` over
/// `*mut RClass` so the C ABI is preserved.
///
/// Construct via `Mrb::define_class` / `Mrb::class_get` (top level),
/// the `Module` trait's `define_class` / `class_get` (nested), or the
/// checked `FromValue` downcast of a class value.
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct RClass(pub(crate) *mut sys::RClass);

/// Typed handle on an mruby module. `#[repr(transparent)]` over
/// `*mut RClass` — mruby models modules with the same C struct as
/// classes; the newtype keeps the distinction at the Rust type level.
///
/// Construct via `Mrb::define_module` (top level), the `Module`
/// trait's `define_module` (nested), or the checked `FromValue`
/// downcast of a module value.
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct RModule(pub(crate) *mut sys::RClass);

/// Typed handle on an exception class — `Exception` itself or an
/// ordinary class descending from it, never a singleton class — so every
/// instance it allocates is an exception, and building one never raises.
/// Mirrors `magnus::ExceptionClass`. `#[repr(transparent)]` over
/// `*mut RClass`.
///
/// Obtain one via `Mrb::exc_get` for a built-in exception class,
/// `Mrb::define_error` / `Module::define_error` for a consumer's own, or
/// `ExceptionClass::from_value` for a class held as a value. Only this
/// handle builds exceptions — the general class handle cannot:
///
/// ```compile_fail
/// # use beni::Mrb;
/// fn build(mrb: &Mrb) {
///     let _ = mrb.object_class().new_str(mrb, mrb.str_new(b"boom"));
/// }
/// ```
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct ExceptionClass(*mut sys::RClass);

// SAFETY: a class handle carries neither the interpreter nor ownership
// of the class it names, so crossing a thread with one is inert; it
// means something only against the interpreter that produced it, the
// same pairing `Value` leaves to the consumer.
unsafe impl Send for RClass {}
unsafe impl Sync for RClass {}
unsafe impl Send for RModule {}
unsafe impl Sync for RModule {}
unsafe impl Send for ExceptionClass {}
unsafe impl Sync for ExceptionClass {}

#[cfg(test)]
mod tests {
    #[test]
    fn class_handles_cross_threads() {
        fn crosses<T: Send + Sync>() {}
        crosses::<crate::RClass>();
        crosses::<crate::RModule>();
        crosses::<crate::ExceptionClass>();
    }
}

mod private {
    use beni_sys as sys;

    /// Plumbing supertrait sealing `Module` / `Object` to the class
    /// handle newtypes and giving their shared default bodies one
    /// raw-pointer accessor.
    pub trait ClassLike: Copy {
        fn raw(self) -> *mut sys::RClass;
    }

    impl ClassLike for super::RClass {
        fn raw(self) -> *mut sys::RClass {
            self.0
        }
    }

    impl ClassLike for super::RModule {
        fn raw(self) -> *mut sys::RClass {
            self.0
        }
    }

    impl ClassLike for super::ExceptionClass {
        fn raw(self) -> *mut sys::RClass {
            self.0
        }
    }
}

/// Derive the mruby aspec from a `method!` wrapper's arity counts:
/// `-1` accepts any arguments (the wrapped function reads the call
/// frame itself), `0..` requires that many positionals, and a
/// non-zero `opt` adds that many optional positionals after them.
/// `block` ORs in the block-accepting flag, which composes with the
/// positional aspec the way mruby's own `MRB_ARGS_ARG` composes its
/// required and optional parts.
fn method_aspec(arity: i8, opt: i8, block: bool) -> sys::mrb_aspec {
    let positional = if arity < 0 {
        sys::mrb_args_any()
    } else if opt > 0 {
        sys::mrb_args_arg(arity as u32, opt as u32)
    } else {
        sys::mrb_args_req(arity as u32)
    };
    if block {
        positional | sys::mrb_args_block()
    } else {
        positional
    }
}

/// Run a registration call inside `Mrb::protect` with the aspec
/// derived from `method.arity` and the typed bridge transmuted to
/// the raw `sys::mrb_func_t` — the single seam where that transmute
/// happens for every `Module` / `Object` registration.
fn protect_register<F>(mrb: &Mrb, method: MethodDef, register: F) -> Result<(), Error>
where
    F: FnOnce(&Mrb, sys::mrb_func_t, sys::mrb_aspec),
{
    mrb.protect(|mrb| {
        let aspec = method_aspec(method.arity, method.opt, method.block);
        // SAFETY: `Value` is `#[repr(transparent)]` over
        // `sys::mrb_value` (pinned by
        // `value::tests::value_shares_abi_with_mrb_value`), so
        // `crate::mrb_func_t` and `sys::mrb_func_t` share C ABI and
        // the transmute is a no-op at codegen.
        let raw: sys::mrb_func_t = unsafe { core::mem::transmute(method.func) };
        register(mrb, raw, aspec);
        crate::value::qnil()
    })
    .map(|_| ())
}

/// Resolve a class definition whose `name` the namespace `outer` itself
/// already binds, as Ruby's `class` keyword resolves a reopened class: the
/// bound ordinary class when `superclass` is its superclass, the
/// `TypeError` otherwise, and `None` for an unbound name the definition
/// then creates. mruby's C definition is not asked here because its fetch
/// yields a prepended class's origin include class instead of the class.
pub(crate) fn bound_class(
    mrb: &Mrb,
    outer: *mut sys::RClass,
    name: crate::Id,
    superclass: RClass,
) -> Option<Result<RClass, Error>> {
    // SAFETY: `outer` names a live class or module of this VM;
    // `mrb_obj_value` only boxes the pointer.
    let outer =
        Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(outer as *mut core::ffi::c_void) });
    if !outer.defines_const_at(mrb, name) {
        return None;
    }
    Some(outer.fetch_const(mrb, name).and_then(|bound| {
        let name = crate::Symbol::from(name).name(mrb).unwrap_or_default();
        let type_error = |message: String| {
            Err(Error::Exception(crate::method::core_exception(
                mrb,
                c"TypeError",
                &message,
            )))
        };
        if bound.tag() != sys::MRB_TT_CLASS {
            return type_error(format!("{name} is not a class"));
        }
        // SAFETY: the class tag was checked just above.
        let class = RClass::from_raw_unchecked(unsafe { bound.as_class_ptr() });
        // SAFETY: `class` is a live class; its `super` link is either
        // null or another class-family struct `mrb_class_real` walks.
        let defined_from =
            RClass::from_raw_unchecked(unsafe { (*class.as_internal()).super_ }).real();
        if defined_from.as_internal() != superclass.as_internal() {
            return type_error(format!("superclass mismatch for class {name}"));
        }
        Ok(class)
    }))
}

/// The type `class` allocates its instances as. Reads the class's flags
/// and never raises.
pub(crate) fn instance_tt(class: *mut sys::RClass) -> sys::mrb_vtype {
    // SAFETY: `class` names a live class; the shim only reads its flag
    // bits.
    unsafe { sys::mrb_instance_tt_func(class) }
}

/// Whether `class` is an exception class — `Exception` itself or a class
/// descending from it, told by the exception instance type `Exception`
/// sets and its descendants inherit. That type is also the one
/// `mrb_exc_new` allocates without raising.
pub(crate) fn is_exception_class(class: *mut sys::RClass) -> bool {
    instance_tt(class) == sys::MRB_TT_EXCEPTION
}

impl RClass {
    /// Wrap a class pointer the crate itself produced.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// The class pointer, for the crate's own calls into `beni::sys` — a
    /// consumer reads it from the class value, as magnus leaves it.
    #[inline]
    pub(crate) const fn as_internal(self) -> *mut sys::RClass {
        self.0
    }

    /// Whether this handle names a singleton class rather than an
    /// ordinary one — the distinction `RClass::from_value` accepts both
    /// sides of. Answers without the `mruby-class-ext` gem that carries
    /// Ruby's `singleton_class?`.
    #[inline]
    pub fn is_singleton(self) -> bool {
        crate::ReprValue::as_value(self).tag() == sys::MRB_TT_SCLASS
    }

    /// `mrb_class_real(self)` — resolve this handle to its real class,
    /// skipping the singleton-class and include-class links a `super`
    /// chain threads through, and yielding the first user-facing class.
    /// A handle that is already a real class returns itself. The
    /// resolution walks the class structure and never raises, so it
    /// needs no exception protection. The normalization a consumer reaches for
    /// after obtaining a handle that may be a singleton class (through
    /// `Object::singleton_class` or `RClass::from_value`); the real-class result
    /// `ReprValue::class` already returns needs no further resolution.
    #[inline]
    pub fn real(self) -> RClass {
        // SAFETY: `mrb_class_real` only walks the `super` chain past
        // singleton / include classes; it reads no `mrb_state` and
        // returns a real class pointer for any live class handle.
        RClass::from_raw_unchecked(unsafe { sys::mrb_class_real(self.0) })
    }

    /// `mrb_obj_new(mrb, self, argc, argv)` — allocate and initialise
    /// a new instance of this class, running `initialize` with `args`.
    /// Surfaces an `Err` when `initialize` raises. Mirrors `magnus`'s
    /// `Class::new_instance`.
    #[inline]
    pub fn new_instance(self, mrb: &Mrb, args: &[Value]) -> Result<Value, Error> {
        // Value is repr(transparent) over mrb_value; the slice
        // pointer reuses the same layout.
        let argv = args.as_ptr() as *const sys::mrb_value;
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` and every `args` entry originate from the same
            // VM. `mrb_obj_new` runs `initialize`, which may raise —
            // caught by `protect`.
            Value::from_raw_unchecked(unsafe {
                sys::mrb_obj_new(
                    mrb.as_ptr(),
                    self.0,
                    sys::mrb_int::try_from(args.len()).unwrap_or(sys::mrb_int::MAX),
                    argv,
                )
            })
        })
    }
}

impl RModule {
    /// Wrap a module pointer the crate itself produced.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// The class pointer, for the crate's own calls into `beni::sys` — a
    /// consumer reads it from the class value, as magnus leaves it.
    #[inline]
    pub(crate) const fn as_internal(self) -> *mut sys::RClass {
        self.0
    }
}

impl ExceptionClass {
    /// Wrap a class pointer the caller has established is an exception
    /// class — a lookup that guarantees it, or a checked downcast.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// The class pointer, for the crate's own calls into `beni::sys` — a
    /// consumer reads it from the class value, as magnus leaves it.
    #[inline]
    pub(crate) const fn as_internal(self) -> *mut sys::RClass {
        self.0
    }

    /// The general class handle on this same class, for the operations
    /// that take any class. Mirrors magnus's `Class::as_r_class`.
    #[inline]
    pub const fn as_r_class(self) -> RClass {
        RClass(self.0)
    }

    /// `mrb_raise(mrb, self, msg)` — raise an exception of this class
    /// with `msg`. Diverges — `mrb_raise` long-jumps out and never
    /// returns to the caller.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind out of (C
    /// bridges, `mrb_funcall` handlers, `mrb_protect_error` bodies).
    /// Calling from arbitrary Rust code would leave Rust frames without
    /// returning through them, so the drops they expect may not run.
    #[inline]
    pub unsafe fn raise(self, mrb: &Mrb, msg: &core::ffi::CStr) -> ! {
        // SAFETY: bridge frame — caller upholds the unwind contract.
        // `mrb_raise` is declared as never returning and the binding
        // carries that, so it satisfies the diverging signature.
        unsafe { sys::mrb_raise(mrb.as_ptr(), self.0, msg.as_ptr()) }
    }

    /// An exception of this class carrying `msg`, built without running
    /// `initialize`; `Error::new` is the public form.
    #[inline]
    pub(crate) fn exc_new(self, mrb: &Mrb, msg: &str) -> Value {
        let len = msg.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
        // SAFETY: `mrb` is alive; `self` is an exception class of the
        // same VM, so the allocation cannot refuse its instance type;
        // `msg`'s bytes are copied into the new exception object
        // before the call returns.
        Value::from_raw_unchecked(unsafe {
            sys::mrb_exc_new(
                mrb.as_ptr(),
                self.0,
                msg.as_ptr() as *const core::ffi::c_char,
                len,
            )
        })
    }

    /// An exception of this class carrying `str` as its message, built
    /// without copying it and, as mruby builds one, without running
    /// `initialize`. Mirrors mruby's `mrb_exc_new_str`.
    #[inline]
    pub fn new_str(self, mrb: &Mrb, str: RString) -> crate::Exception {
        // SAFETY: `mrb` is alive; `self` is an exception class and `str`
        // a String-tagged value of the same VM, so neither the
        // allocation nor the string type guard can raise.
        let exc = Value::from_raw_unchecked(unsafe {
            sys::mrb_exc_new_str(mrb.as_ptr(), self.0, str.as_raw())
        });
        // SAFETY: an exception class allocates exception objects.
        unsafe { <crate::Exception as crate::value::private::ReprValue>::from_value_unchecked(exc) }
    }
}

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
            .map(|class| ExceptionClass::from_raw_unchecked(class.as_internal()))
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

    /// `mrb_define_const_id(mrb, self, name, val)` — bind the constant
    /// `name` to `val` on this class or module. The name is a
    /// symbol-or-name key (`IntoId`). Runs inside exception protection, so a
    /// frozen-receiver rejection surfaces as `Err(Error::Exception)`
    /// rather than long-jumping — the same contract as the definition
    /// methods above.
    fn define_const<K: IntoId>(self, mrb: &Mrb, name: K, val: Value) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` and `val` originate from the same VM; `sym`
            // was interned against it.
            unsafe { sys::mrb_define_const_id(mrb.as_ptr(), self.raw(), sym, val.as_raw()) };
            crate::value::qnil()
        })
        .map(|_| ())
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

    /// `mrb_class_name(mrb, self)` — the handle's full Ruby name as an
    /// owned `String` (e.g. `"MyService::KV"`), synthesizing a
    /// `#<Class:0x…>` form for an anonymous handle. mruby builds the
    /// name into a GC-managed temporary, so the bytes are copied out at
    /// once rather than borrowed.
    fn name(self, mrb: &Mrb) -> String {
        // SAFETY: `mrb` is alive by the borrow; `self` originates
        // from the same VM by the single-VM contract.
        let ptr = unsafe { sys::mrb_class_name(mrb.as_ptr(), self.raw()) };
        if ptr.is_null() {
            return String::new();
        }
        // SAFETY: `ptr` is a valid C string for the duration of this
        // call; copy its bytes before the temporary it points into
        // can be collected.
        unsafe { core::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("")
            .to_owned()
    }

    /// `mrb_class_path(mrb, self)` — the handle's fully-qualified path,
    /// the namespace chain leading to it (`"Outer::Inner"` for a nested
    /// class, the bare name for a top-level one), or `None` when the
    /// handle is anonymous and has no place in any namespace. A total
    /// read that never raises. Unlike `name`, which always answers a name —
    /// synthesizing a `#<Class:0x…>` form for an anonymous handle — `path`
    /// answers the qualified path or nothing; both return an owned `String`
    /// copied out of mruby's freshly built string.
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

/// Per-object surface of an instance-variable holder — beni's mirror of
/// `magnus::Object`: singleton methods, instance variables, and the
/// singleton class. Implemented only by the handles whose object mruby
/// lets hold instance variables (`obj_iv_p`), which is narrower than
/// CRuby's set: a string, array, or other built-in layout carries none.
///
/// A handle outside that set does not implement the trait:
///
/// ```compile_fail
/// fn holder<T: beni::Object>() {}
/// holder::<beni::RString>();
/// ```
///
/// ```compile_fail
/// fn holder<T: beni::Object>() {}
/// holder::<beni::Value>();
/// ```
pub trait Object: ReprValue {
    /// `mrb_define_singleton_method_id(mrb, self, name, func, aspec)` —
    /// register a method on this object's singleton class from a
    /// `method!`-wrapped Rust function. The name is a symbol-or-name
    /// key (`IntoId`). A singleton method on a class is its class method.
    fn define_singleton_method<K: IntoId>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: as `Module::define_method`; every holder is a heap
            // object, so `mrb_obj_ptr` reads its `RObject *` header,
            // which mruby's singleton-class preparation accepts.
            unsafe {
                sys::mrb_define_singleton_method_id(
                    mrb.as_ptr(),
                    sys::mrb_obj_ptr_func(self.as_value().0),
                    sym,
                    raw,
                    aspec,
                )
            };
        })
    }

    /// Undefine a singleton method on this object, through its singleton
    /// class: the per-object counterpart of `Module::undef_method`. The
    /// name is a symbol-or-name key (`IntoId`); undefining a name absent
    /// from the object surfaces as `Err(Error::Exception)` (mruby's
    /// `NameError`).
    fn undef_singleton_method<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM; `sym` was interned against
            // it. `mrb_singleton_class` yields a class-tagged value or
            // raises, and `mrb_undef_method_id` raises NameError when
            // the method is absent — both caught by `protect`.
            unsafe {
                let singleton = sys::mrb_singleton_class(mrb.as_ptr(), self.as_value().0);
                sys::mrb_undef_method_id(mrb.as_ptr(), sys::mrb_class_ptr_func(singleton), sym);
            }
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_iv_get(mrb, self, sym)` — read instance variable `name` from
    /// `self` through `TryConvert` into `U`; an unset variable reads as
    /// `nil`. Surfaces only the key's or the conversion's `Err`.
    #[inline]
    fn ivar_get<K: IntoId, U: TryConvert>(self, mrb: &Mrb, name: K) -> Result<U, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        // SAFETY: `mrb` is alive; `self` is a holder of the same VM, and
        // `mrb_iv_get` reads its table without dispatching.
        let value = mrb.hold(Value(unsafe {
            sys::mrb_iv_get(mrb.as_ptr(), self.as_value().0, sym)
        }));
        U::try_convert(value, mrb)
    }

    /// `mrb_iv_set(mrb, self, sym, val)` — assign instance variable
    /// `name` on `self` to `val`. Surfaces an `Err` when `self` is frozen.
    #[inline]
    fn ivar_set<K: IntoId, U: IntoValue>(self, mrb: &Mrb, name: K, val: U) -> Result<(), Error> {
        let sym = name.into_id(mrb)?.to_raw();
        let val = val.into_value(mrb);
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self` and
            // `val` originate from the same VM. `mrb_iv_set` raises
            // `FrozenError` on a frozen holder — caught by `protect`.
            unsafe { sys::mrb_iv_set(mrb.as_ptr(), self.as_value().0, sym, val.0) };
            crate::value::qnil()
        })
        .map(|_| ())
    }

    /// `mrb_iv_defined(mrb, self, sym)` — TRUE when instance variable
    /// `name` is set on `self`. A name too long to be a symbol answers
    /// false; `mrb_obj_iv_defined`, the raw-`RObject*` form, stays in
    /// `sys`.
    #[inline]
    fn ivar_defined<K: IntoId>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_id(mrb).map(crate::Id::to_raw) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` is a holder of the same VM.
        unsafe { sys::mrb_iv_defined(mrb.as_ptr(), self.as_value().0, sym) }
    }

    /// `mrb_iv_remove(mrb, self, sym)` — remove instance variable `name`
    /// from `self`, returning `Some` of its former value, or `None` when
    /// it is absent — distinct from a variable removed while holding
    /// `nil`. Surfaces an `Err` only when `self` is frozen.
    #[inline]
    fn ivar_remove<K: IntoId>(self, mrb: &Mrb, name: K) -> Result<Option<Value>, Error> {
        let sym = name.into_id(mrb)?.to_raw();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_iv_remove` raises
            // `FrozenError` on a frozen holder — caught by `protect`.
            Value(unsafe { sys::mrb_iv_remove(mrb.as_ptr(), self.as_value().0, sym) })
        })
        .map(|removed| {
            // SAFETY: a total tag read on the protected result.
            if unsafe { sys::mrb_undef_p_func(removed.0) } {
                None
            } else {
                Some(removed)
            }
        })
    }

    /// `mrb_iv_foreach(mrb, self, …)` — visit each instance variable set
    /// on `self` in iv-table order, handing its name as a typed `Symbol`
    /// and its value to `body`. Returning `ForEach::Stop` ends the
    /// iteration before the remaining variables; `ForEach::Continue`
    /// proceeds. The iteration visits the variables and the values they
    /// held when it began: `body` reassigning, removing, or adding the
    /// holder's instance variables changes the holder but never the
    /// visited set, and each visited value holds arena protection as if
    /// created here, staying valid across `body`'s own mutations and
    /// collections. The iteration dispatches no Ruby and so never raises;
    /// magnus binds no ivar foreach, so this anchors on mruby's own
    /// `mrb_iv_foreach`.
    ///
    /// A panic in `body` ends the iteration and propagates; `body` runs
    /// after the C walk has finished, so the panic never crosses
    /// mruby's frames.
    #[inline]
    fn ivar_foreach<F>(self, mrb: &Mrb, body: F)
    where
        F: FnMut(crate::Symbol, Value) -> crate::ForEach,
    {
        // Snapshot the (name, value) pairs before any caller code
        // runs: the C foreach walks the live iv table, which `body`
        // re-entering the VM could free and reallocate mid-walk, so
        // `body` only ever runs against this collected copy. Each
        // value is arena-protected as it is collected — the holder
        // stops referencing a value `body` removes, and the snapshot
        // must outlive any collection `body` triggers.
        unsafe extern "C" fn collect(
            mrb: *mut sys::mrb_state,
            name: sys::mrb_sym,
            val: sys::mrb_value,
            data: *mut core::ffi::c_void,
        ) -> core::ffi::c_int {
            // SAFETY: `data` is the `&mut Vec<…>` handed to
            // `mrb_iv_foreach` below, borrowed for the duration of
            // the walk on this same thread; `mrb` is the live state
            // driving the walk, and protecting into the arena leaves
            // the iv table untouched.
            let pairs: &mut Vec<(crate::Symbol, Value)> =
                unsafe { &mut *(data as *mut Vec<(crate::Symbol, Value)>) };
            unsafe { sys::mrb_gc_protect(mrb, val) };
            pairs.push((
                crate::Symbol::from(crate::Id::from_raw_unchecked(name)),
                Value::from_raw_unchecked(val),
            ));
            0
        }

        let mut pairs: Vec<(crate::Symbol, Value)> = Vec::new();
        // SAFETY: `mrb` is alive; `self` originates from the same VM.
        // `collect` upholds the `mrb_iv_foreach_func` ABI and runs no
        // caller code; `data` points to `pairs` on this frame, which
        // outlives the call. bindgen wraps the function-typedef
        // parameter in `Option`, so the collector is passed via `Some`.
        unsafe {
            sys::mrb_iv_foreach(
                mrb.as_ptr(),
                self.as_value().0,
                Some(collect),
                &mut pairs as *mut Vec<(crate::Symbol, Value)> as *mut core::ffi::c_void,
            );
        }
        let mut body = body;
        for (name, val) in pairs {
            if let crate::ForEach::Stop = body(name, val) {
                break;
            }
        }
    }

    /// `mrb_singleton_class(mrb, self)` — the holder's own singleton
    /// class, Ruby's `singleton_class`: the per-instance eigenclass that
    /// holds methods defined on that one object, distinct from the
    /// regular class `ReprValue::class` returns. Created on first read
    /// and stable across re-reads. Surfaces an `Err` carrying a
    /// `TypeError` only where mruby gives the object no singleton class.
    /// The raw `RClass*` form (`mrb_singleton_class_ptr`) stays behind
    /// `beni::sys`.
    #[inline]
    fn singleton_class(self, mrb: &Mrb) -> Result<RClass, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_singleton_class` raises
            // `TypeError` when the object has no singleton class —
            // caught by `protect` — and otherwise returns a
            // class-tagged value.
            let v = Value::from_raw_unchecked(unsafe {
                sys::mrb_singleton_class(mrb.as_ptr(), self.as_value().0)
            });
            // SAFETY: a value returned without a raise is class-tagged,
            // so the pointer recovery accepts it.
            RClass::from_raw_unchecked(unsafe { v.as_class_ptr() })
        })
    }
}

impl Object for RClass {}
impl Object for RModule {}
impl Object for ExceptionClass {}
impl Object for crate::RObject {}
impl Object for crate::RHash {}
impl Object for crate::RTypedData {}
impl<T: crate::TypedData> Object for crate::typed_data::Obj<T> {}
impl Object for crate::Exception {}

crate::value::class_backed_repr!(RClass);
crate::value::class_backed_repr!(RModule);
crate::value::class_backed_repr!(ExceptionClass);

impl FromValue for RClass {
    // A singleton class is a class handle too — `Object::singleton_class`
    // hands one out — so both class tags convert.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (class or singleton-class
        // tagging) is established by the guard immediately before it.
        matches!(value.tag(), sys::MRB_TT_CLASS | sys::MRB_TT_SCLASS)
            .then(|| RClass::from_raw_unchecked(unsafe { value.as_class_ptr() }))
    }
}

impl FromValue for RModule {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the unbox precondition (MRB_TT_MODULE tagging) is
        // established by the tag check immediately before it.
        (value.tag() == sys::MRB_TT_MODULE)
            .then(|| RModule::from_raw_unchecked(unsafe { value.as_class_ptr() }))
    }
}

impl FromValue for ExceptionClass {
    // Narrower than its tag: a class converts only when it is an
    // exception class, which a singleton class never is.
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        if value.tag() != sys::MRB_TT_CLASS {
            return None;
        }
        // SAFETY: the unbox precondition (class tagging) is established
        // by the tag check immediately above.
        let class = unsafe { value.as_class_ptr() };
        crate::class::is_exception_class(class).then(|| ExceptionClass::from_raw_unchecked(class))
    }
}

/// The `TryConvert` of a class handle, naming the kind of class it
/// expects in the `TypeError`.
macro_rules! try_convert_class {
    ($($handle:ty => $kind:literal),* $(,)?) => {$(
        impl TryConvert for $handle {
            #[inline]
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                <$handle>::from_value(val).ok_or_else(|| {
                    crate::try_convert::type_error(mrb, &format!("{} is not {}", val.inspect(mrb), $kind))
                })
            }
        }
    )*};
}

try_convert_class!(
    RClass => "a class",
    RModule => "a module",
    ExceptionClass => "a class inheriting Exception",
);
