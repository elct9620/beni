//! Typed `RClass` / `RModule` / `ExceptionClass` handles and the
//! `Module` / `Object` registration traits — beni's mirror of
//! `magnus::RClass` / `magnus::RModule` / `magnus::ExceptionClass` with
//! `magnus::Module` / `magnus::Object`.
//!
//! ## Why newtypes
//!
//! Same rationale as `Value`: the raw `*mut RClass` pointer crosses
//! the crate boundary, and consumers historically had to pass it
//! around untyped — easy to leak, easy to confuse with other opaque
//! pointers, and impossible to attach inherent methods to from a
//! sibling crate. mruby represents classes and modules with the same
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

use crate::{Error, IntoSym, MethodDef, Mrb, RString, Value};
use beni_sys as sys;

/// Typed handle on an mruby class. `#[repr(transparent)]` over
/// `*mut RClass` so the C ABI is preserved.
///
/// Construct via `Mrb::define_class` / `Mrb::class_get` (top level),
/// the `Module` trait's `define_class` / `class_get` (nested), or
/// `RClass::from_raw` at FFI boundaries.
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct RClass(pub(crate) *mut sys::RClass);

/// Typed handle on an mruby module. `#[repr(transparent)]` over
/// `*mut RClass` — mruby models modules with the same C struct as
/// classes; the newtype keeps the distinction at the Rust type level.
///
/// Construct via `Mrb::define_module` (top level), the `Module`
/// trait's `define_module` (nested), or `RModule::from_raw` at FFI
/// boundaries.
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
///     let _ = mrb.object_class().exc_new(mrb, "boom");
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
        Value::nil()
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
    name: crate::Symbol,
    superclass: RClass,
) -> Option<Result<RClass, Error>> {
    // SAFETY: `outer` names a live class or module of this VM;
    // `mrb_obj_value` only boxes the pointer.
    let outer =
        Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(outer as *mut core::ffi::c_void) });
    if !outer.const_defined_at(mrb, name) {
        return None;
    }
    Some(outer.const_get(mrb, name).and_then(|bound| {
        let name = name.name(mrb).unwrap_or_default();
        let type_error = |message: String| {
            Err(Error::Exception(crate::method::core_exception(
                mrb,
                c"TypeError",
                &message,
            )))
        };
        if !bound.is_class() {
            return type_error(format!("{name} is not a class"));
        }
        // SAFETY: the class tag was checked just above.
        let class = RClass::from_raw_unchecked(unsafe { bound.as_class_ptr() });
        // SAFETY: `class` is a live class; its `super` link is either
        // null or another class-family struct `mrb_class_real` walks.
        let defined_from = RClass::from_raw_unchecked(unsafe { (*class.as_raw()).super_ }).real();
        if defined_from.as_raw() != superclass.as_raw() {
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
    /// Wrap a raw `*mut RClass` a bridge received from mruby directly.
    /// Most call sites get the pointer from the typed definition methods
    /// instead. A class pointer has no crossing trait of its own the way
    /// a value and an id do: a class is a value in CRuby, so magnus
    /// offers only the checked downcast and leaves nothing to mirror.
    ///
    /// # Safety
    ///
    /// `p` must be a live class of the interpreter it is used against.
    /// The typed surface dereferences it rather than testing it.
    #[inline]
    pub const unsafe fn from_raw(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Wrap a class pointer the crate itself produced — the internal
    /// counterpart of the `unsafe` crossing above.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Borrow the inner `*mut RClass` for raw FFI calls. The wrapper
    /// itself stays usable after the borrow (`RClass: Copy`).
    #[inline]
    pub const fn as_raw(self) -> *mut sys::RClass {
        self.0
    }

    /// TRUE when the underlying pointer is null. Only reachable via
    /// `RClass::from_raw` on a NULL pointer — the typed lookup paths
    /// surface missing classes as `Err` instead.
    #[inline]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// `mrb_class_real(self)` — resolve this handle to its real class,
    /// skipping the singleton-class and include-class links a `super`
    /// chain threads through, and yielding the first user-facing class.
    /// A handle that is already a real class returns itself. The
    /// resolution walks the class structure and never raises, so it
    /// needs no exception protection. The normalization a consumer reaches for
    /// after obtaining a handle that may be a singleton class (through
    /// `Value::singleton_class` or `RClass::from_value`) or an include
    /// class (through `RClass::from_raw`); the real-class result
    /// `Value::class` already returns needs no further resolution.
    #[inline]
    pub fn real(self) -> RClass {
        // SAFETY: `mrb_class_real` only walks the `super` chain past
        // singleton / include classes; it reads no `mrb_state` and
        // returns a real class pointer for any live class handle.
        RClass::from_raw_unchecked(unsafe { sys::mrb_class_real(self.0) })
    }

    /// `mrb_obj_value(self)` — the `Value` naming this class, for the
    /// value-level APIs (constants, dispatch, singleton class) that take
    /// any value. Raises nothing and runs no Ruby.
    ///
    /// Named `to_value`, not `as_value`: `RClass` wraps a `*mut RClass`
    /// pointer, so the value is boxed through mruby's `mrb_obj_value`
    /// rather than read out of a field the way the `Value`-newtype
    /// handles (`Array` / `Symbol` / `Proc`) expose it as `as_value`.
    #[inline]
    pub fn to_value(self, _mrb: &Mrb) -> Value {
        // SAFETY: `self` names a class of this VM — the pairing every
        // handle method relies on; `mrb_obj_value` only boxes the
        // pointer.
        Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(self.0 as *mut core::ffi::c_void) })
    }

    /// `mrb_obj_new(mrb, self, argc, argv)` — allocate and initialise
    /// a new instance of this class, running `initialize` with `args`.
    /// Surfaces an `Err` when `initialize` raises. Mirrors `magnus`'s
    /// `Class::new_instance`.
    #[inline]
    pub fn obj_new(self, mrb: &Mrb, args: &[Value]) -> Result<Value, Error> {
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
    /// Wrap a raw `*mut RClass` known to be a module. Counterpart of
    /// `RClass::from_raw` for FFI boundaries.
    ///
    /// # Safety
    ///
    /// As `RClass::from_raw`, and `p` must name a module.
    #[inline]
    pub const unsafe fn from_raw(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Wrap a module pointer the crate itself produced — the internal
    /// counterpart of the `unsafe` crossing above.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Borrow the inner `*mut RClass` for raw FFI calls. The wrapper
    /// itself stays usable after the borrow (`RModule: Copy`).
    #[inline]
    pub const fn as_raw(self) -> *mut sys::RClass {
        self.0
    }

    /// `mrb_obj_value(self)` — the `Value` naming this module; the
    /// counterpart of `RClass::to_value`. Raises nothing and runs no
    /// Ruby.
    #[inline]
    pub fn to_value(self, _mrb: &Mrb) -> Value {
        // SAFETY: as `RClass::to_value`.
        Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(self.0 as *mut core::ffi::c_void) })
    }
}

impl ExceptionClass {
    /// Wrap a class pointer the caller has established is an exception
    /// class — a lookup that guarantees it, or a checked downcast.
    #[inline]
    pub(crate) const fn from_raw_unchecked(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Borrow the inner `*mut RClass` for raw FFI calls. The wrapper
    /// itself stays usable after the borrow (`ExceptionClass: Copy`).
    #[inline]
    pub const fn as_raw(self) -> *mut sys::RClass {
        self.0
    }

    /// The general class handle on this same class, for the operations
    /// that take any class. Mirrors magnus's `Class::as_r_class`.
    #[inline]
    pub const fn as_r_class(self) -> RClass {
        RClass(self.0)
    }

    /// `mrb_obj_value(self)` — the `Value` naming this exception class;
    /// the counterpart of `RClass::to_value`. Raises nothing and runs no
    /// Ruby.
    #[inline]
    pub fn to_value(self, _mrb: &Mrb) -> Value {
        // SAFETY: as `RClass::to_value`.
        Value::from_raw_unchecked(unsafe { sys::mrb_obj_value(self.0 as *mut core::ffi::c_void) })
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

    /// `mrb_exc_new(mrb, self, msg, len)` — build an exception of this
    /// class carrying `msg`, without raising it. The bytes are copied
    /// into the new object before the call returns. Counterpart to
    /// `ExceptionClass::raise` for the path that returns the exception as
    /// a `Value` — a bridge body wraps it in `Error::Exception` to raise
    /// it to the Ruby caller at the boundary instead of long-jumping
    /// mid-body. Building never raises: the class allocates exceptions.
    /// `msg.len()` saturates to `sys::mrb_int::MAX` (the archive's
    /// configured integer width), like `Mrb::str_new`; real handler
    /// messages stay far below that.
    #[inline]
    pub fn exc_new(self, mrb: &Mrb, msg: &str) -> Value {
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

    /// `mrb_exc_new_str(mrb, self, str)` — build an exception of this
    /// class carrying an existing mruby string as its message, without
    /// raising it. The counterpart to `ExceptionClass::exc_new` for a
    /// message a consumer already holds as an `RString` (one it built,
    /// mutated, or received), carried as-is with no Rust-side copy and no
    /// re-encoding through bytes — distinct from `exc_new`, which
    /// allocates a fresh string from Rust bytes. Building never raises and
    /// runs no user Ruby: the class allocates exceptions, and the
    /// `RString` is statically a string.
    #[inline]
    pub fn exc_new_str(self, mrb: &Mrb, str: RString) -> Value {
        // SAFETY: `mrb` is alive; `self` is an exception class and `str`
        // a String-tagged value of the same VM, so neither the
        // allocation nor the string type guard can raise.
        Value::from_raw_unchecked(unsafe {
            sys::mrb_exc_new_str(mrb.as_ptr(), self.0, str.as_raw())
        })
    }
}

/// Registration surface shared by classes and modules — beni's
/// mirror of `magnus::Module`. Every method runs inside
/// exception protection, so an mruby raise surfaces as
/// `Err(Error::Exception)` and never unwinds across FFI.
pub trait Module: private::ClassLike {
    /// `mrb_define_class_under_id(mrb, self, name, superclass)` —
    /// define (or fetch) the nested class `self::name` inheriting from
    /// `superclass`. The name is a symbol-or-name key (`IntoSym`). A
    /// name `self` already binds yields that ordinary class itself when
    /// `superclass` is its superclass, prepended modules and all, and a
    /// `TypeError` for anything else bound there.
    fn define_class<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        superclass: RClass,
    ) -> Result<RClass, Error> {
        let sym = name.into_sym(mrb)?;
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
                    sym.to_sym(),
                    superclass.as_raw(),
                )
            })
        })
    }

    /// Define (or fetch) the nested exception class `self::name`
    /// descending from `superclass`, yielding it as an `ExceptionClass`.
    /// Mirrors magnus's `Module::define_error`; rejected as
    /// `Module::define_class` is.
    fn define_error<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        superclass: ExceptionClass,
    ) -> Result<ExceptionClass, Error> {
        // A class defined or fetched under an exception-class superclass
        // descends from it, so it is an exception class too.
        self.define_class(mrb, name, superclass.as_r_class())
            .map(|class| ExceptionClass::from_raw_unchecked(class.as_raw()))
    }

    /// `mrb_define_module_under_id(mrb, self, name)` — define (or
    /// fetch) the nested module `self::name`. The name is a
    /// symbol-or-name key (`IntoSym`). mruby rejects a same-named
    /// constant that is not a module.
    fn define_module<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<RModule, Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RModule::from_raw_unchecked(unsafe {
                sys::mrb_define_module_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_class_get_under_id(mrb, self, name)` — fetch the nested
    /// class `self::name`. The name is a symbol-or-name key
    /// (`IntoSym`). mruby raises `NameError` when the constant is
    /// missing and `TypeError` when it is not a class (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    fn class_get<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<RClass, Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RClass::from_raw_unchecked(unsafe {
                sys::mrb_class_get_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_module_get_under_id(mrb, self, name)` — fetch the nested
    /// module `self::name`. The name is a symbol-or-name key
    /// (`IntoSym`). mruby raises `NameError` when the constant is
    /// missing and `TypeError` when it is not a module (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    fn module_get<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<RModule, Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: as `define_class`.
            RModule::from_raw_unchecked(unsafe {
                sys::mrb_module_get_under_id(mrb.as_ptr(), self.raw(), sym)
            })
        })
    }

    /// `mrb_class_defined_under_id(mrb, self, name)` — TRUE when a
    /// class or module is defined under `self::name`. The name is a
    /// symbol-or-name key (`IntoSym`), routed through the `_id` form
    /// like `class_get`. A total predicate: an undefined name reads
    /// `false` rather than raising, so it is the precondition test
    /// before a namespaced fetching lookup that would raise on a
    /// missing name. A name too long to intern can never be bound, so
    /// it reads `false` too.
    fn class_defined<K: IntoSym>(self, mrb: &Mrb, name: K) -> bool {
        let Ok(sym) = name.into_sym(mrb) else {
            return false;
        };
        // SAFETY: `mrb` is alive; `self` originates from the same
        // VM; `sym` was interned against it. `mrb_class_defined_under_id`
        // is a constant-existence lookup that does not raise.
        unsafe { sys::mrb_class_defined_under_id(mrb.as_ptr(), self.raw(), sym.to_sym()) }
    }

    /// `mrb_define_method_id(mrb, self, name, func, aspec)` — register
    /// an instance method from a `method!`-wrapped Rust function. The
    /// name is a symbol-or-name key (`IntoSym`). The aspec is derived
    /// from the wrapper's arity (`-1` = any arguments, `0..` = that
    /// many required positionals). mruby rejects registration on a
    /// frozen receiver.
    fn define_method<K: IntoSym>(self, mrb: &Mrb, name: K, method: MethodDef) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
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
    /// name is a symbol-or-name key (`IntoSym`). The aspec derivation
    /// and rejection contract match `define_method`.
    fn define_private_method<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
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
    /// symbol-or-name key (`IntoSym`). The aspec derivation and
    /// rejection contract match `define_method`.
    fn define_module_function<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
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
    /// symbol-or-name key (`IntoSym`). Runs inside exception protection, so a
    /// frozen-receiver rejection surfaces as `Err(Error::Exception)`
    /// rather than long-jumping — the same contract as the definition
    /// methods above.
    fn define_const<K: IntoSym>(self, mrb: &Mrb, name: K, val: Value) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` and `val` originate from the same VM; `sym`
            // was interned against it.
            unsafe { sys::mrb_define_const_id(mrb.as_ptr(), self.raw(), sym, val.as_raw()) };
            Value::nil()
        })
        .map(|_| ())
    }

    /// `mrb_define_alias_id(mrb, self, new, old)` — bind `new` as a
    /// second name for the existing method `old` on this class or module,
    /// so a core method can be preserved before it is overridden. Both
    /// names are symbol-or-name keys (`IntoSym`), each interned to its
    /// symbol before the `_id` alias call. Runs inside exception protection, so
    /// aliasing a method that does not exist surfaces as
    /// `Err(Error::Exception)` (mruby's `NameError`) rather than
    /// long-jumping — the same contract as the definition methods above.
    fn alias_method<N: IntoSym, O: IntoSym>(self, mrb: &Mrb, new: N, old: O) -> Result<(), Error> {
        let new = new.into_sym(mrb)?.to_sym();
        let old = old.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `new` and `old`
            // were interned against it. The original-method lookup
            // raises NameError when `old` is absent — caught by
            // `protect`.
            unsafe { sys::mrb_define_alias_id(mrb.as_ptr(), self.raw(), new, old) };
            Value::nil()
        })
        .map(|_| ())
    }

    /// `mrb_undef_method_id(mrb, self, name)` — undefine a method on
    /// this class or module, Ruby's `Module#undef_method`: the name is
    /// marked as not defined on the handle even when an ancestor defines
    /// it. The name is a symbol-or-name key (`IntoSym`); both forms
    /// resolve to the same interned symbol and route through the raising
    /// `_id` C function, so undefining a name absent from the handle and
    /// its ancestors surfaces as `Err(Error::Exception)` (mruby's
    /// `NameError`) rather than long-jumping — the same contract as the
    /// definition methods above.
    fn undef_method<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `sym` was interned
            // against it. `mrb_undef_method_id` raises NameError when
            // the method is absent — caught by `protect`.
            unsafe { sys::mrb_undef_method_id(mrb.as_ptr(), self.raw(), sym) };
            Value::nil()
        })
        .map(|_| ())
    }

    /// `mrb_remove_method(mrb, self, name)` — remove a method from this
    /// class or module, Ruby's `Module#remove_method`: the method's own
    /// definition is deleted from the handle, so the name reverts to any
    /// ancestor's method — distinct from `undef_method`, which masks
    /// ancestor lookups rather than stripping the definition. The name is a
    /// symbol-or-name key (`IntoSym`). Removing a name not defined directly
    /// on the handle raises `NameError`; under exception protection it surfaces as
    /// `Err(Error::Exception)` rather than long-jumping — the same contract
    /// as the definition methods above.
    fn remove_method<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `sym` was interned
            // against it. `mrb_remove_method` raises NameError when the
            // method is not defined on the handle — caught by `protect`.
            unsafe { sys::mrb_remove_method(mrb.as_ptr(), self.raw(), sym) };
            Value::nil()
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
            unsafe { sys::mrb_include_module(mrb.as_ptr(), self.raw(), module.as_raw()) };
            Value::nil()
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
            unsafe { sys::mrb_prepend_module(mrb.as_ptr(), self.raw(), module.as_raw()) };
            Value::nil()
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
}

impl Module for RClass {}
impl Module for RModule {}
impl Module for ExceptionClass {}

/// Per-object registration surface — beni's mirror of
/// `magnus::Object`, currently covering singleton-method
/// registration on the two handle newtypes.
pub trait Object: private::ClassLike {
    /// `mrb_define_singleton_method_id(mrb, self, name, func, aspec)` —
    /// register a singleton-class method on this handle from a
    /// `method!`-wrapped Rust function. The name is a symbol-or-name
    /// key (`IntoSym`). The receiver is treated as `RObject *` so the
    /// singleton-class shim attaches to the metaclass (matching mruby's
    /// own contract). A class or module always carries a singleton class,
    /// so the registration installs the method on its metaclass.
    fn define_singleton_method<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        protect_register(mrb, method, |mrb, raw, aspec| {
            // SAFETY: as `Module::define_method`; the `RClass *` →
            // `RObject *` cast mirrors mruby's own
            // `mrb_define_class_method_id`, which casts `(struct
            // RObject*)c` before this same call.
            unsafe {
                sys::mrb_define_singleton_method_id(
                    mrb.as_ptr(),
                    self.raw() as *mut sys::RObject,
                    sym,
                    raw,
                    aspec,
                )
            };
        })
    }

    /// `mrb_undef_class_method_id(mrb, self, name)` — undefine a
    /// singleton method on this handle: the class-method counterpart of
    /// `Module::undef_method`'s instance form, since a class's singleton
    /// method is its class method. The name is a symbol-or-name
    /// key (`IntoSym`), routed through the raising `_id` C function, so
    /// undefining a singleton name absent from the handle surfaces as
    /// `Err(Error::Exception)` (mruby's `NameError`).
    fn undef_singleton_method<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<(), Error> {
        let sym = name.into_sym(mrb)?.to_sym();
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM; `sym` was interned
            // against it. `mrb_undef_class_method_id` resolves the
            // singleton class and raises NameError when the method is
            // absent — caught by `protect`.
            unsafe { sys::mrb_undef_class_method_id(mrb.as_ptr(), self.raw(), sym) };
            Value::nil()
        })
        .map(|_| ())
    }
}

impl Object for RClass {}
impl Object for RModule {}
impl Object for ExceptionClass {}
