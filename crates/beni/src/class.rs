//! Typed `RClass` / `RModule` handles and the `Module` / `Object`
//! registration traits — beni's mirror of `magnus::RClass` /
//! `magnus::RModule` with `magnus::Module` / `magnus::Object`.
//!
//! ## Why newtypes
//!
//! Same rationale as `Value`: the raw `*mut RClass` pointer crosses
//! the crate boundary, and consumers historically had to pass it
//! around untyped — easy to leak, easy to confuse with other opaque
//! pointers, and impossible to attach inherent methods to from a
//! sibling crate. mruby represents classes and modules with the same
//! C `struct RClass`; the two Rust newtypes keep "this handle is a
//! class" / "this handle is a module" distinct at the type level
//! while sharing the registration surface through the traits.
//!
//! ## ABI guarantee
//!
//! Both handles are `#[repr(transparent)]` over `*mut RClass`, so
//! they are pointer-sized and share the C ABI on every target — a
//! struct field of either type round-trips into mruby's own
//! `RClass *` slot without conversion.
//!
//! ## Error contract
//!
//! Every definition, registration, and lookup runs inside
//! `Mrb::protect`, so an mruby raise (superclass mismatch, frozen
//! receiver, missing constant, …) surfaces as `Err(Error::Exception)`
//! instead of long-jumping across Rust frames.

use crate::{Error, MethodDef, Mrb, Value};
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

mod private {
    use beni_sys as sys;

    /// Plumbing supertrait sealing `Module` / `Object` to the two
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
}

/// Derive the mruby aspec from a `method!` wrapper's arity: `-1`
/// accepts any arguments (the wrapped function reads the call frame
/// itself), `0..` requires that many positionals.
#[cfg(mruby_linked)]
fn method_aspec(arity: i8) -> sys::mrb_aspec {
    if arity < 0 {
        sys::mrb_args_any()
    } else {
        sys::mrb_args_req(arity as u32)
    }
}

/// Run a registration call inside `Mrb::protect` with the aspec
/// derived from `method.arity` and the typed bridge transmuted to
/// the raw `sys::mrb_func_t` — the single seam where that transmute
/// happens for every `Module` / `Object` registration.
#[cfg(mruby_linked)]
fn protect_register<F>(mrb: &Mrb, method: MethodDef, register: F) -> Result<(), Error>
where
    F: FnOnce(&Mrb, sys::mrb_func_t, sys::mrb_aspec),
{
    mrb.protect(|mrb| {
        let aspec = method_aspec(method.arity);
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

/// Run `f` inside `Mrb::protect`, boxing the class/module pointer it
/// produces as a `Value` to ride through the protect frame and
/// unboxing it on the way out — the shared plumbing behind every
/// definition and lookup that yields a handle. An mruby raise inside
/// `f` surfaces as `Err(Error::Exception)`.
#[cfg(mruby_linked)]
pub(crate) fn protect_class_ptr<F>(mrb: &Mrb, f: F) -> Result<*mut sys::RClass, Error>
where
    F: FnOnce(&Mrb) -> *mut sys::RClass,
{
    mrb.protect(|mrb| {
        let raw = f(mrb);
        // SAFETY: `raw` was just produced against the live VM by the
        // closure's definition/lookup call.
        Value::from_raw(unsafe { sys::mrb_obj_value(raw as *mut core::ffi::c_void) })
    })
    // SAFETY: the Ok value boxes the pointer produced above.
    .map(|v| unsafe { v.as_class_ptr() })
}

impl RClass {
    /// Wrap a raw `*mut RClass` produced by FFI. Most call sites get
    /// the pointer from the typed definition methods; `from_raw`
    /// serves bridges that receive one from mruby directly.
    #[inline]
    pub const fn from_raw(p: *mut sys::RClass) -> Self {
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

    /// Reify this class handle as an mruby `Value` via mruby's own
    /// `mrb_obj_value` (an `MRB_INLINE` reached through bindgen's
    /// static-fn trampoline). Used by call paths that need to pass
    /// the class through generic mruby APIs that accept `mrb_value`
    /// (e.g. `mrb_const_defined` / `mrb_const_get` /
    /// `Object#constants`).
    ///
    /// Named `to_value`, not `as_value`: `RClass` wraps a `*mut RClass`
    /// pointer, so reification is an `mrb_obj_value` call against the
    /// live VM, not the free field read the `Value`-newtype handles
    /// (`Array` / `Symbol` / `Proc`) expose as `as_value`.
    ///
    /// # Safety
    ///
    /// `self` must be a live class handle produced by the same VM
    /// as `mrb` (and not yet freed).
    #[inline]
    pub unsafe fn to_value(self, _mrb: &Mrb) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller; mrb_obj_value reads only
            // the pointer payload and reuses mruby's own boxing logic.
            Value::from_raw(unsafe { sys::mrb_obj_value(self.0 as *mut core::ffi::c_void) })
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_obj_new(mrb, self, argc, argv)` — allocate and initialise
    /// a new instance of this class, calling `initialize` with `args`.
    /// A raising `initialize` long-jumps — only call from contexts
    /// that can absorb one (C bridges, `Mrb::protect` bodies).
    #[inline]
    pub fn obj_new(self, mrb: &Mrb, args: &[Value]) -> Value {
        #[cfg(mruby_linked)]
        {
            // Value is repr(transparent) over mrb_value; the slice
            // pointer reuses the same layout.
            let argv = args.as_ptr() as *const sys::mrb_value;
            // SAFETY: `mrb` is alive; `self` and every `args` entry
            // originate from the same VM.
            Value::from_raw(unsafe {
                sys::mrb_obj_new(mrb.as_ptr(), self.0, args.len() as sys::mrb_int, argv)
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, args);
            crate::not_linked()
        }
    }

    /// `mrb_raise(mrb, self, msg)` — raise an exception of this class
    /// with `msg`. Diverges — `mrb_raise` long-jumps out and never
    /// returns to the caller.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind out of (C
    /// bridges, `mrb_funcall` handlers, `mrb_protect_error` bodies).
    /// Calling from arbitrary Rust code would skip Rust drop frames
    /// the stack expects to run.
    #[inline]
    pub unsafe fn raise(self, mrb: &Mrb, msg: &core::ffi::CStr) -> ! {
        #[cfg(mruby_linked)]
        {
            // SAFETY: bridge frame — caller upholds the unwind contract.
            // bindgen drops the `mrb_noreturn` attribute on its `mrb_raise`
            // declaration, so the FFI return type is `()` rather than the
            // diverging `!`. The `unreachable_unchecked` keeps the
            // diverging Rust signature without an extra runtime branch —
            // `mrb_raise` long-jumps before control can reach it.
            unsafe { sys::mrb_raise(mrb.as_ptr(), self.0, msg.as_ptr()) };
            unsafe { core::hint::unreachable_unchecked() }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, msg);
            crate::not_linked()
        }
    }

    /// `mrb_exc_new(mrb, self, msg, len)` — build an exception of this
    /// class carrying `msg`, without raising it. The bytes are copied
    /// into the new object before the call returns. Counterpart to
    /// `RClass::raise` for the path that returns the exception as a
    /// `Value` — a bridge body wraps it in `Error::Exception` to raise
    /// it to the Ruby caller at the boundary instead of long-jumping
    /// mid-body.
    #[inline]
    pub fn exc_new(self, mrb: &Mrb, msg: &str) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` originates from the same
            // VM; `msg`'s bytes are copied into the new exception
            // object before the call returns.
            Value::from_raw(unsafe {
                sys::mrb_exc_new(
                    mrb.as_ptr(),
                    self.0,
                    msg.as_ptr() as *const core::ffi::c_char,
                    msg.len() as sys::mrb_int,
                )
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, msg);
            crate::not_linked()
        }
    }
}

impl RModule {
    /// Wrap a raw `*mut RClass` known to be a module. Counterpart of
    /// `RClass::from_raw` for FFI boundaries.
    #[inline]
    pub const fn from_raw(p: *mut sys::RClass) -> Self {
        Self(p)
    }

    /// Borrow the inner `*mut RClass` for raw FFI calls. The wrapper
    /// itself stays usable after the borrow (`RModule: Copy`).
    #[inline]
    pub const fn as_raw(self) -> *mut sys::RClass {
        self.0
    }
}

/// Registration surface shared by classes and modules — beni's
/// mirror of `magnus::Module`. Every method runs inside
/// `Mrb::protect`, so an mruby raise surfaces as
/// `Err(Error::Exception)` and never unwinds across FFI.
pub trait Module: private::ClassLike {
    /// `mrb_define_class_under(mrb, self, name, superclass)` — define
    /// (or fetch) the nested class `self::name` inheriting from
    /// `superclass`. mruby rejects a superclass mismatch with an
    /// existing definition, or a same-named constant that is not a
    /// class.
    fn define_class(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        superclass: RClass,
    ) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `superclass` originate from the same VM;
                // `name` is NUL-terminated.
                unsafe {
                    sys::mrb_define_class_under(
                        mrb.as_ptr(),
                        self.raw(),
                        name.as_ptr(),
                        superclass.as_raw(),
                    )
                }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, superclass);
            crate::not_linked()
        }
    }

    /// `mrb_define_module_under(mrb, self, name)` — define (or fetch)
    /// the nested module `self::name`. mruby rejects a same-named
    /// constant that is not a module.
    fn define_module(self, mrb: &Mrb, name: &core::ffi::CStr) -> Result<RModule, Error> {
        #[cfg(mruby_linked)]
        {
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: as `define_class`.
                unsafe { sys::mrb_define_module_under(mrb.as_ptr(), self.raw(), name.as_ptr()) }
            })
            .map(RModule::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name);
            crate::not_linked()
        }
    }

    /// `mrb_class_get_under(mrb, self, name)` — fetch the nested
    /// class `self::name`. mruby raises `NameError` when the constant
    /// is missing and `TypeError` when it is not a class
    /// (vendored `src/class.c` documents both), so the lookup is
    /// fallible by contract.
    fn class_get(self, mrb: &Mrb, name: &core::ffi::CStr) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: as `define_class`.
                unsafe { sys::mrb_class_get_under(mrb.as_ptr(), self.raw(), name.as_ptr()) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name);
            crate::not_linked()
        }
    }

    /// `mrb_define_method(mrb, self, name, func, aspec)` — register
    /// an instance method from a `method!`-wrapped Rust function.
    /// The aspec is derived from the wrapper's arity
    /// (`-1` = any arguments, `0..` = that many required
    /// positionals). mruby rejects registration on a frozen receiver.
    fn define_method(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        method: MethodDef,
    ) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` was produced by the same VM; `name` is
                // NUL-terminated; `raw` has the C ABI mruby expects.
                unsafe {
                    sys::mrb_define_method(mrb.as_ptr(), self.raw(), name.as_ptr(), raw, aspec)
                };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
    }

    /// `mrb_define_private_method(mrb, self, name, func, aspec)` —
    /// like `define_method`, with private visibility: Ruby-level
    /// dispatch with an explicit receiver raises `NoMethodError`.
    /// The aspec derivation and rejection contract match
    /// `define_method`.
    fn define_private_method(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        method: MethodDef,
    ) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: as `define_method` — same signature, same
                // contract.
                unsafe {
                    sys::mrb_define_private_method(
                        mrb.as_ptr(),
                        self.raw(),
                        name.as_ptr(),
                        raw,
                        aspec,
                    )
                };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
    }

    /// `mrb_define_const(mrb, self, name, val)` — bind the constant
    /// `name` to `val` on this class or module. Runs inside
    /// `Mrb::protect`, so a frozen-receiver rejection surfaces as
    /// `Err(Error::Exception)` rather than long-jumping — the same
    /// contract as the definition methods above.
    fn define_const(self, mrb: &Mrb, name: &core::ffi::CStr, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `val` originate from the same VM; `name`
                // is NUL-terminated.
                unsafe {
                    sys::mrb_define_const(mrb.as_ptr(), self.raw(), name.as_ptr(), val.as_raw())
                };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, val);
            crate::not_linked()
        }
    }

    /// `mrb_class_name(mrb, self)` — the handle's full Ruby name
    /// (e.g. `"MyService::KV"`). Returns `None` when mruby yields
    /// NULL. The returned slice points into mruby's interned
    /// class-name storage which lives for the duration of the VM.
    fn name(self, mrb: &Mrb) -> Option<&'static str> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the borrow; `self` originates
            // from the same VM by the single-VM contract.
            let ptr = unsafe { sys::mrb_class_name(mrb.as_ptr(), self.raw()) };
            if ptr.is_null() {
                return None;
            }
            // SAFETY: mruby's class-name storage lives for the duration
            // of the VM.
            Some(
                unsafe { core::ffi::CStr::from_ptr(ptr) }
                    .to_str()
                    .unwrap_or(""),
            )
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}

impl Module for RClass {}
impl Module for RModule {}

/// Per-object registration surface — beni's mirror of
/// `magnus::Object`, currently covering singleton-method
/// registration on the two handle newtypes.
pub trait Object: private::ClassLike {
    /// `mrb_define_singleton_method(mrb, self, name, func, aspec)` —
    /// register a singleton-class method on this handle from a
    /// `method!`-wrapped Rust function. The receiver is treated
    /// as `RObject *` so the singleton-class shim
    /// attaches to the metaclass (matching mruby's own contract).
    /// mruby rejects receivers that cannot carry a singleton class.
    fn define_singleton_method(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        method: MethodDef,
    ) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: as `Module::define_method`; `RClass *` and
                // `RObject *` are both `c_void *` aliases in this
                // crate's binding, and the cast matches what
                // `mrbgems/mruby-singleton-class` does inline.
                unsafe {
                    sys::mrb_define_singleton_method(
                        mrb.as_ptr(),
                        self.raw() as *mut sys::RObject,
                        name.as_ptr(),
                        raw,
                        aspec,
                    )
                };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
    }
}

impl Object for RClass {}
impl Object for RModule {}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;

    /// Registration target answering a fixed Integer for the trait
    /// tests below.
    fn answer_seven(_mrb: &Mrb, _self: Value) -> i32 {
        7
    }

    fn answer_nine(_mrb: &Mrb, _self: Value) -> i32 {
        9
    }

    #[test]
    fn define_class_surfaces_mruby_rejection_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        let base = mrb
            .define_class(c"BeniErrBase", object)
            .expect("defining the base class must succeed");
        mrb.define_class(c"BeniErrChild", base)
            .expect("defining the child class must succeed");

        // Redefining with a different superclass is the documented
        // E_TYPE_ERROR rejection (vendored src/class.c superclass
        // mismatch) — it must surface as Err, not a longjmp.
        let err = mrb
            .define_class(c"BeniErrChild", object)
            .expect_err("superclass mismatch must surface as Err");
        assert!(matches!(err, Error::Exception(_)));
        assert!(
            err.message(&mrb).contains("superclass mismatch"),
            "unexpected rejection message: {}",
            err.message(&mrb)
        );
    }

    #[test]
    fn class_get_surfaces_name_error_for_missing_class() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // mruby raises NameError for a missing constant (vendored
        // src/class.c documents the lookup contract) — the typed
        // lookup must catch it instead of long-jumping.
        let err = mrb
            .class_get(c"BeniNoSuchClass")
            .expect_err("missing class must surface as Err");
        assert!(
            err.message(&mrb).contains("BeniNoSuchClass"),
            "the NameError must name the missing constant: {}",
            err.message(&mrb)
        );
    }

    #[test]
    fn private_method_rejects_public_dispatch_but_is_attached() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        let class = mrb
            .define_class(c"BeniPrivate", object)
            .expect("defining the class must succeed");
        class
            .define_private_method(&mrb, c"secret", crate::method!(answer_seven, 0))
            .expect("registering the private method must succeed");
        let receiver = class.obj_new(&mrb, &[]);

        // VM-dispatched code with an explicit receiver must observe
        // the visibility (the funcall path bypasses it by design):
        // OP_SEND raises NoMethodError for a private method.
        // SAFETY: `mrb` is alive; the code literal is NUL-terminated.
        // `mrb_load_string` absorbs the raise into `mrb->exc`.
        let _ = unsafe { sys::mrb_load_string(mrb.as_ptr(), c"BeniPrivate.new.secret".as_ptr()) };
        let exc = mrb.pending_exc();
        assert!(
            !exc.is_nil(),
            "public dispatch of a private method must raise"
        );
        let message = Error::Exception(exc).message(&mrb);
        assert!(
            message.contains("private"),
            "the NoMethodError must name the visibility: {message}"
        );
        mrb.clear_exc();

        // mrb_funcall bypasses visibility, confirming the body is
        // attached and runs.
        let got = mrb
            .protect(|mrb| receiver.call(mrb, c"secret", &[]))
            .expect("funcall dispatch must reach the private body");
        assert_eq!(unsafe { got.unbox_integer() }, 7);
    }

    #[test]
    fn module_and_object_traits_register_methods() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        // Module trait: nested definition + instance-method
        // registration, exercised end-to-end through a Ruby call.
        let outer = mrb
            .define_module(c"BeniTrait")
            .expect("defining the module must succeed");
        let class = outer
            .define_class(&mrb, c"Widget", object)
            .expect("defining the nested class must succeed");
        assert_eq!(class.name(&mrb), Some("BeniTrait::Widget"));

        class
            .define_method(&mrb, c"answer", crate::method!(answer_seven, 0))
            .expect("registering the instance method must succeed");
        let receiver = class.obj_new(&mrb, &[]);
        let got = receiver.call(&mrb, c"answer", &[]);
        assert_eq!(unsafe { got.unbox_integer() }, 7);

        // Object trait: singleton registration on the class handle,
        // invoked through the reified class value.
        class
            .define_singleton_method(&mrb, c"class_answer", crate::method!(answer_nine, 0))
            .expect("registering the singleton method must succeed");
        // SAFETY: `class` is a live handle from this VM.
        let class_value = unsafe { class.to_value(&mrb) };
        let got = class_value.call(&mrb, c"class_answer", &[]);
        assert_eq!(unsafe { got.unbox_integer() }, 9);

        // Lookup round-trip through the trait.
        let fetched = outer
            .class_get(&mrb, c"Widget")
            .expect("fetching the nested class must succeed");
        assert_eq!(fetched.name(&mrb), Some("BeniTrait::Widget"));
    }

    #[test]
    fn define_const_binds_a_constant_readable_from_ruby() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let module = mrb
            .define_module(c"BeniConstHost")
            .expect("defining the host module must succeed");

        module
            .define_const(&mrb, c"ANSWER", Value::from_int(&mrb, 42))
            .expect("binding the constant must succeed");

        // The constant must resolve from plain Ruby source — the
        // consumer-visible end of the binding.
        let cxt = crate::Ccontext::new(&mrb, c"const_test.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"BeniConstHost::ANSWER");
        assert!(
            mrb.pending_exc().is_nil(),
            "reading the constant must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert!(got.is_integer(), "the bound constant reads back as Integer");
        assert_eq!(unsafe { got.unbox_integer() }, 42);
    }

    #[test]
    fn exc_new_builds_an_exception_of_the_class_without_raising() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let runtime_error = mrb
            .class_get(c"RuntimeError")
            .expect("RuntimeError is present in every VM");

        let exc = runtime_error.exc_new(&mrb, "something failed");

        // Building does not raise; the object carries the class and the
        // message verbatim, ready to ride out as Error::Exception.
        assert!(mrb.pending_exc().is_nil(), "exc_new must not raise");
        assert_eq!(exc.classname(&mrb), "RuntimeError");
        assert_eq!(Error::Exception(exc).message(&mrb), "something failed");
    }
}
