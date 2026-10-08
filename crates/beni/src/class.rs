//! Typed `RClass` / `RModule` / `ExceptionClass` handles — beni's mirror
//! of `magnus::RClass` / `magnus::RModule` / `magnus::ExceptionClass`.
//! The surface they share lives on the `Module` and `Object` traits, and
//! what only a class handle does on the `Class` trait.
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
    sys::AsRawValue, Error, FromValue, MethodDef, Mrb, RString, ReprValue, TryConvert, Value,
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

pub(crate) mod private {
    use beni_sys as sys;

    /// Plumbing supertrait sealing `Module` / `Object` / `Class` to the class
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
pub(crate) fn method_aspec(arity: i8, opt: i8, block: bool) -> sys::mrb_aspec {
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
pub(crate) fn protect_register<F>(mrb: &Mrb, method: MethodDef, register: F) -> Result<(), Error>
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
        let type_error = |message: String| Err(crate::try_convert::type_error(mrb, &message));
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

/// Whether mruby allocates an object of type `tt` against `class` without
/// raising: the class's instances are of that type, or the class is
/// `Object`, which also allocates data carriers and inline structs
/// (`vendor/mruby/src/gc.c:573-580`). Answered from flag reads alone, so a
/// wrap refuses a class before allocating instead of catching the raise.
pub(crate) fn allocates_as(mrb: &Mrb, class: RClass, tt: sys::mrb_vtype) -> bool {
    instance_tt(class.as_internal()) == tt
        || class.as_internal() == mrb.object_class().as_internal()
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

    /// The exception class `class` is, for a class the caller has
    /// established descends from an exception class — one defined from
    /// or under an exception-class superclass.
    #[inline]
    pub(crate) const fn from_descendant_unchecked(class: RClass) -> Self {
        Self(class.0)
    }

    /// The exception `instance` is, for a value an exception class
    /// allocated or constructed.
    #[inline]
    fn exception_unchecked(instance: Value) -> crate::Exception {
        // SAFETY: an exception class allocates exception objects.
        unsafe {
            <crate::Exception as crate::value::private::ReprValue>::from_value_unchecked(instance)
        }
    }

    /// The class pointer, for the crate's own calls into `beni::sys` — a
    /// consumer reads it from the class value, as magnus leaves it.
    #[inline]
    pub(crate) const fn as_internal(self) -> *mut sys::RClass {
        self.0
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
        Self::exception_unchecked(Value::from_raw_unchecked(unsafe {
            sys::mrb_exc_new_str(mrb.as_ptr(), self.0, str.as_raw())
        }))
    }
}

/// Allocate an instance of `class` as mruby's `mrb_instance_alloc`
/// (`vendor/mruby/src/class.c`) does for `new` and `allocate`. Its
/// refusals are checked here in its order and answered as an `Err`, so
/// `mrb_obj_alloc` meets only a class it allocates for and raises nothing
/// but the out-of-memory error every allocation can.
fn alloc_instance(mrb: &Mrb, class: RClass) -> Result<Value, Error> {
    let refuse = |message: String| Err(crate::try_convert::type_error(mrb, &message));
    if class.is_singleton() {
        return refuse("can't create instance of singleton class".to_owned());
    }
    let state = mrb.as_ptr();
    let raw = class.0;
    let mut tt = instance_tt(raw);
    // SAFETY: `state` is the live interpreter borrowed as `mrb`; the read
    // copies two class pointers out of it.
    let nil_or_false = unsafe { raw == (*state).nil_class || raw == (*state).false_class };
    if tt == sys::MRB_TT_FALSE && !nil_or_false {
        tt = sys::MRB_TT_OBJECT;
    }
    // SAFETY: `raw` names a live class; the shim only reads a flag bit.
    if unsafe { sys::mrb_undef_allocator_p_func(raw) } {
        return refuse(format!("allocator undefined for {}", class.name(mrb)));
    }
    if tt <= sys::MRB_TT_CPTR {
        return refuse(format!("can't create instance of {}", class.name(mrb)));
    }
    // SAFETY: `state` is alive; `raw` is a plain class whose instances are
    // heap objects of type `tt`, the one type `mrb_obj_alloc` accepts for
    // it, and the new object is kept in the arena as every value
    // factory's is.
    let object = unsafe { sys::mrb_obj_alloc(state, tt, raw) };
    // SAFETY: `object` is the live object just allocated.
    Ok(Value::from_raw_unchecked(unsafe {
        sys::mrb_obj_value(object as *mut core::ffi::c_void)
    }))
}

/// Operations on a class handle that a module handle has no use for —
/// beni's mirror of `magnus::Class`, implemented by `RClass` and
/// `ExceptionClass`. An mruby raise or refusal surfaces as
/// `Err(Error::Exception)` and never unwinds across FFI.
pub trait Class: crate::Module {
    /// The handle an instance of the class comes back as: a `Value` for
    /// any class, an `Exception` for an exception class.
    type Instance;

    /// `mrb_class_new(mrb, superclass)` — create an anonymous class
    /// inheriting from `superclass`, bound to no constant. The class
    /// gains a name only when later bound to a constant. mruby rejects a
    /// singleton class or `Class` itself as the superclass.
    fn new(mrb: &Mrb, superclass: Self) -> Result<Self, Error>;

    /// `mrb_obj_new(mrb, self, argc, argv)` — construct an instance as
    /// Ruby's built-in `Class#new` does: allocate as `obj_alloc` does,
    /// then run `initialize` with `args` and no block. The class's own
    /// `new` is never called. Surfaces an `Err` when allocation is
    /// refused or `initialize` raises.
    fn new_instance(self, mrb: &Mrb, args: &[Value]) -> Result<Self::Instance, Error>;

    /// Allocate an instance of this class without running `initialize`,
    /// the instance mruby's built-in `allocate` makes. Dispatches no Ruby,
    /// so neither a Ruby-defined `allocate` nor `new` is called. A
    /// singleton class, a class whose default allocator is undefined, or
    /// one whose instances are of an immediate or C-pointer type refuses
    /// with mruby's `TypeError`.
    fn obj_alloc(self, mrb: &Mrb) -> Result<Self::Instance, Error>;

    /// The class Ruby's `Class#superclass` answers for this one: its
    /// parent, past the include classes that modules included or
    /// prepended along the chain add, or `None` for `BasicObject`.
    /// Answers an `Option` where magnus answers a `Result`: mruby's read
    /// never raises and answers `nil` only at the chain's end
    /// (`vendor/mruby/src/class.c`, `mrb_class_superclass`). The class
    /// stays reachable as every value that crosses out does.
    fn superclass(self, mrb: &Mrb) -> Option<RClass> {
        // SAFETY: `self` is a live class handle; the shim only follows
        // its `super` chain, which the class keeps reachable.
        let parent = unsafe { sys::mrb_class_superclass_func(self.raw()) };
        (!parent.is_null()).then(|| {
            let parent = RClass::from_raw_unchecked(parent);
            mrb.hold(parent.as_value());
            parent
        })
    }

    /// `mrb_class_name(mrb, self)` — the class's full Ruby name (e.g.
    /// `"MyService::KV"`), synthesizing a `#<Class:0x…>` form for a class
    /// with no path. Safe and owned where magnus's borrow is `unsafe`:
    /// mruby builds the name into a GC-managed temporary, so the bytes
    /// are copied out at once and nothing borrows from the VM.
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

    /// The general class handle on this same class, for the operations
    /// that take any class.
    fn as_r_class(self) -> RClass {
        RClass::from_raw_unchecked(self.raw())
    }

    /// Undefine the default allocator of this class and of any class
    /// later defined from it, so Ruby's `new` and `allocate` raise while
    /// wraps still allocate. A singleton class, which Ruby never
    /// allocates through, is left unchanged.
    fn undef_default_alloc_func(self, mrb: &Mrb) {
        // The `&Mrb` borrow is what serializes this flag write: a class
        // handle crosses threads on its own, its interpreter does not.
        let _ = mrb;
        if self.as_value().tag() == sys::MRB_TT_CLASS {
            // SAFETY: `self` is a live plain class of the VM borrowed as
            // `mrb`, the one kind `MRB_UNDEF_ALLOCATOR` accepts; the shim
            // only sets a flag bit.
            unsafe { sys::mrb_undef_allocator_func(self.raw()) };
        }
    }
}

impl Class for RClass {
    type Instance = Value;

    #[inline]
    fn new(mrb: &Mrb, superclass: Self) -> Result<Self, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `superclass` was produced by the same VM.
            RClass::from_raw_unchecked(unsafe { sys::mrb_class_new(mrb.as_ptr(), superclass.0) })
        })
    }

    #[inline]
    fn new_instance(self, mrb: &Mrb, args: &[Value]) -> Result<Value, Error> {
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

    #[inline]
    fn obj_alloc(self, mrb: &Mrb) -> Result<Value, Error> {
        alloc_instance(mrb, self)
    }

    #[inline]
    fn as_r_class(self) -> RClass {
        self
    }
}

impl Class for ExceptionClass {
    type Instance = crate::Exception;

    #[inline]
    fn new(mrb: &Mrb, superclass: Self) -> Result<Self, Error> {
        // A class defined from an exception class inherits its exception
        // instance type, so it is an exception class too.
        RClass::new(mrb, superclass.as_r_class()).map(ExceptionClass::from_descendant_unchecked)
    }

    #[inline]
    fn new_instance(self, mrb: &Mrb, args: &[Value]) -> Result<crate::Exception, Error> {
        self.as_r_class()
            .new_instance(mrb, args)
            .map(Self::exception_unchecked)
    }

    #[inline]
    fn obj_alloc(self, mrb: &Mrb) -> Result<crate::Exception, Error> {
        alloc_instance(mrb, self.as_r_class()).map(Self::exception_unchecked)
    }
}

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
