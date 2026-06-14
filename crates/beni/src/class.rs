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
//! Every definition, registration, lookup, and instance construction
//! runs inside `Mrb::protect`, so an mruby raise (superclass mismatch,
//! frozen receiver, missing constant, a raising `initialize`, …)
//! surfaces as `Err(Error::Exception)` instead of long-jumping across
//! Rust frames.

use crate::{Error, IntoSym, MethodDef, Mrb, Value};
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
    /// a new instance of this class, running `initialize` with `args`.
    /// Surfaces an `Err` when `initialize` raises. Mirrors `magnus`'s
    /// `Class::new_instance`.
    #[inline]
    pub fn obj_new(self, mrb: &Mrb, args: &[Value]) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            // Value is repr(transparent) over mrb_value; the slice
            // pointer reuses the same layout.
            let argv = args.as_ptr() as *const sys::mrb_value;
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and every `args` entry originate from the same
                // VM. `mrb_obj_new` runs `initialize`, which may raise —
                // caught by `protect`.
                Value::from_raw(unsafe {
                    sys::mrb_obj_new(mrb.as_ptr(), self.0, args.len() as sys::mrb_int, argv)
                })
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
    /// mid-body. `msg.len()` saturates to `sys::mrb_int::MAX` (the
    /// archive's configured integer width), like `Mrb::str_new`; real
    /// handler messages stay far below that.
    #[inline]
    pub fn exc_new(self, mrb: &Mrb, msg: &str) -> Value {
        #[cfg(mruby_linked)]
        {
            let len = msg.len().min(sys::mrb_int::MAX as usize) as sys::mrb_int;
            // SAFETY: `mrb` is alive; `self` originates from the same
            // VM; `msg`'s bytes are copied into the new exception
            // object before the call returns.
            Value::from_raw(unsafe {
                sys::mrb_exc_new(
                    mrb.as_ptr(),
                    self.0,
                    msg.as_ptr() as *const core::ffi::c_char,
                    len,
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
    /// `mrb_define_class_under_id(mrb, self, name, superclass)` —
    /// define (or fetch) the nested class `self::name` inheriting from
    /// `superclass`. The name is a symbol-or-name key (`IntoSym`),
    /// resolved to its symbol before the `_id` definition call. mruby
    /// rejects a superclass mismatch with an existing definition, or a
    /// same-named constant that is not a class.
    fn define_class<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        superclass: RClass,
    ) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `superclass` originate from the same VM;
                // `sym` was interned against the same VM.
                unsafe {
                    sys::mrb_define_class_under_id(
                        mrb.as_ptr(),
                        self.raw(),
                        sym,
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

    /// `mrb_define_module_under_id(mrb, self, name)` — define (or
    /// fetch) the nested module `self::name`. The name is a
    /// symbol-or-name key (`IntoSym`). mruby rejects a same-named
    /// constant that is not a module.
    fn define_module<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<RModule, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: as `define_class`.
                unsafe { sys::mrb_define_module_under_id(mrb.as_ptr(), self.raw(), sym) }
            })
            .map(RModule::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name);
            crate::not_linked()
        }
    }

    /// `mrb_class_get_under_id(mrb, self, name)` — fetch the nested
    /// class `self::name`. The name is a symbol-or-name key
    /// (`IntoSym`). mruby raises `NameError` when the constant is
    /// missing and `TypeError` when it is not a class (vendored
    /// `src/class.c` documents both), so the lookup is fallible by
    /// contract.
    fn class_get<K: IntoSym>(self, mrb: &Mrb, name: K) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_class_ptr(mrb, |mrb| {
                // SAFETY: as `define_class`.
                unsafe { sys::mrb_class_get_under_id(mrb.as_ptr(), self.raw(), sym) }
            })
            .map(RClass::from_raw)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name);
            crate::not_linked()
        }
    }

    /// `mrb_define_method_id(mrb, self, name, func, aspec)` — register
    /// an instance method from a `method!`-wrapped Rust function. The
    /// name is a symbol-or-name key (`IntoSym`). The aspec is derived
    /// from the wrapper's arity (`-1` = any arguments, `0..` = that
    /// many required positionals). mruby rejects registration on a
    /// frozen receiver.
    fn define_method<K: IntoSym>(self, mrb: &Mrb, name: K, method: MethodDef) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` was produced by the same VM; `sym` was
                // interned against it; `raw` has the C ABI mruby expects.
                unsafe { sys::mrb_define_method_id(mrb.as_ptr(), self.raw(), sym, raw, aspec) };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: as `define_method` — same signature, same
                // contract.
                unsafe {
                    sys::mrb_define_private_method_id(mrb.as_ptr(), self.raw(), sym, raw, aspec)
                };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
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
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: as `define_method` — same signature, same
                // contract.
                unsafe {
                    sys::mrb_define_module_function_id(mrb.as_ptr(), self.raw(), sym, raw, aspec)
                };
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, method.func, method.arity);
            crate::not_linked()
        }
    }

    /// `mrb_define_const_id(mrb, self, name, val)` — bind the constant
    /// `name` to `val` on this class or module. The name is a
    /// symbol-or-name key (`IntoSym`). Runs inside `Mrb::protect`, so a
    /// frozen-receiver rejection surfaces as `Err(Error::Exception)`
    /// rather than long-jumping — the same contract as the definition
    /// methods above.
    fn define_const<K: IntoSym>(self, mrb: &Mrb, name: K, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `val` originate from the same VM; `sym`
                // was interned against it.
                unsafe { sys::mrb_define_const_id(mrb.as_ptr(), self.raw(), sym, val.as_raw()) };
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

    /// `mrb_define_alias(mrb, self, new, old)` — bind `new` as a second
    /// name for the existing method `old` on this class or module, so a
    /// core method can be preserved before it is overridden. Runs inside
    /// `Mrb::protect`, so aliasing a method that does not exist surfaces
    /// as `Err(Error::Exception)` (mruby's `NameError`) rather than
    /// long-jumping — the same contract as the definition methods above.
    fn alias_method(
        self,
        mrb: &Mrb,
        new: &core::ffi::CStr,
        old: &core::ffi::CStr,
    ) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` originates from the same VM; `new` and `old`
                // are NUL-terminated.
                unsafe {
                    sys::mrb_define_alias(mrb.as_ptr(), self.raw(), new.as_ptr(), old.as_ptr())
                };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, new, old);
            crate::not_linked()
        }
    }

    /// `mrb_include_module(mrb, self, module)` — mix `module` into this
    /// class or module, Ruby's `include`. A frozen receiver raises
    /// `FrozenError` and a cyclic include raises `ArgumentError`; both
    /// surface as `Err` via `Mrb::protect`.
    fn include_module(self, mrb: &Mrb, module: RModule) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, module);
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
    /// `mrb_define_singleton_method_id(mrb, self, name, func, aspec)` —
    /// register a singleton-class method on this handle from a
    /// `method!`-wrapped Rust function. The name is a symbol-or-name
    /// key (`IntoSym`). The receiver is treated as `RObject *` so the
    /// singleton-class shim attaches to the metaclass (matching mruby's
    /// own contract). mruby rejects receivers that cannot carry a
    /// singleton class.
    fn define_singleton_method<K: IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        method: MethodDef,
    ) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            protect_register(mrb, method, |mrb, raw, aspec| {
                // SAFETY: as `Module::define_method`; `RClass *` and
                // `RObject *` are both `c_void *` aliases in this
                // crate's binding, and the cast matches what
                // `mrbgems/mruby-singleton-class` does inline.
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
    fn symbol_key_reaches_the_same_definition_as_the_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        // Defining with an already-interned Symbol must reach the same
        // definition path as a name key: the class is then fetchable by
        // its plain name, and a method registered under a Symbol key is
        // callable.
        let name = crate::Symbol::new(&mrb, c"BeniSymKeyed");
        let class = mrb
            .define_class(name, object)
            .expect("defining the class under a Symbol key must succeed");
        class
            .define_method(
                &mrb,
                crate::Symbol::new(&mrb, c"answer"),
                crate::method!(answer_seven, 0),
            )
            .expect("registering a method under a Symbol key must succeed");
        class
            .define_const(
                &mrb,
                crate::Symbol::new(&mrb, c"ANSWER"),
                Value::from_int(&mrb, 7),
            )
            .expect("binding a constant under a Symbol key must succeed");

        // Fetch by Symbol key resolves to the class defined above.
        let fetched = mrb
            .class_get(crate::Symbol::new(&mrb, c"BeniSymKeyed"))
            .expect("fetching by a Symbol key must reach the defined class");
        assert_eq!(fetched.name(&mrb), Some("BeniSymKeyed"));

        // The method and constant keyed by Symbol read back through the
        // equivalent name — both keys resolve to the same interned sym.
        let receiver = fetched
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"answer", &[])
            .expect("the Symbol-keyed method must be callable by name");
        assert_eq!(unsafe { got.unbox_integer() }, 7);

        // SAFETY: `fetched` is a live handle from this VM.
        let const_val = unsafe { fetched.to_value(&mrb) }
            .const_get(&mrb, mrb.intern_cstr(c"ANSWER"))
            .expect("the Symbol-keyed constant must read by name");
        assert_eq!(unsafe { const_val.unbox_integer() }, 7);
    }

    #[test]
    fn symbol_key_and_name_key_are_interchangeable_for_lookup() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        // A class defined by name is fetchable by Symbol, and one defined
        // by Symbol is fetchable by name — the two key forms are
        // interchangeable because both resolve to the same interned sym.
        mrb.define_class(c"BeniByName", object)
            .expect("defining by name must succeed");
        let by_sym = mrb
            .class_get(crate::Symbol::new(&mrb, c"BeniByName"))
            .expect("a name-defined class is fetchable by Symbol key");
        assert_eq!(by_sym.name(&mrb), Some("BeniByName"));
    }

    #[test]
    fn symbol_key_registers_private_singleton_and_module_function() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        // Each registration variant routes its Symbol key through the
        // matching `_id` C function; a method registered under a Symbol
        // is then reachable, proving the key reached the definition.
        let class = mrb
            .define_class(c"BeniSymVariants", object)
            .expect("defining the class must succeed");
        class
            .define_private_method(
                &mrb,
                crate::Symbol::new(&mrb, c"secret"),
                crate::method!(answer_seven, 0),
            )
            .expect("registering the private method under a Symbol key must succeed");
        class
            .define_singleton_method(
                &mrb,
                crate::Symbol::new(&mrb, c"klass_answer"),
                crate::method!(answer_nine, 0),
            )
            .expect("registering the singleton method under a Symbol key must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        // funcall bypasses visibility, reaching the private body.
        let private = receiver
            .funcall(&mrb, c"secret", &[])
            .expect("the Symbol-keyed private method must be reachable via funcall");
        assert_eq!(unsafe { private.unbox_integer() }, 7);
        // SAFETY: `class` is a live handle from this VM.
        let singleton = unsafe { class.to_value(&mrb) }
            .funcall(&mrb, c"klass_answer", &[])
            .expect("the Symbol-keyed singleton method must be callable");
        assert_eq!(unsafe { singleton.unbox_integer() }, 9);

        let module = mrb
            .define_module(c"BeniSymModFn")
            .expect("defining the module must succeed");
        module
            .define_module_function(
                &mrb,
                crate::Symbol::new(&mrb, c"mod_seven"),
                crate::method!(answer_seven, 0),
            )
            .expect("registering the module function under a Symbol key must succeed");

        // The module function is callable as a singleton on the module
        // object — the consumer-visible end of the Symbol-keyed
        // registration.
        let cxt = crate::Ccontext::new(&mrb, c"sym_modfn.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"BeniSymModFn.mod_seven");
        assert!(
            mrb.pending_exc().is_nil(),
            "calling the Symbol-keyed module function must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(unsafe { got.unbox_integer() }, 7);
    }

    #[test]
    fn nested_definition_accepts_a_symbol_key() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        // The namespaced Module-trait define/get also accept a Symbol key.
        let outer = mrb
            .define_module(crate::Symbol::new(&mrb, c"BeniSymNs"))
            .expect("defining the module under a Symbol key must succeed");
        let nested = outer
            .define_class(&mrb, crate::Symbol::new(&mrb, c"Inner"), object)
            .expect("defining the nested class under a Symbol key must succeed");
        assert_eq!(nested.name(&mrb), Some("BeniSymNs::Inner"));

        let fetched = outer
            .class_get(&mrb, crate::Symbol::new(&mrb, c"Inner"))
            .expect("fetching the nested class under a Symbol key must succeed");
        assert_eq!(fetched.name(&mrb), Some("BeniSymNs::Inner"));
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
    fn obj_new_surfaces_a_raising_initialize_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = crate::Ccontext::new(&mrb, c"obj_new_test.rb")
            .expect("allocating the context must succeed");

        cxt.load_nstring(b"class BeniBoomInit; def initialize; raise 'no'; end; end");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise"
        );

        // Constructing runs the raising initialize — surfaced as Err
        // instead of long-jumping across the call.
        let class = mrb
            .class_get(c"BeniBoomInit")
            .expect("the class is defined");
        assert!(matches!(class.obj_new(&mrb, &[]), Err(Error::Exception(_))));
    }

    #[test]
    fn registering_onto_a_frozen_class_surfaces_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = crate::Ccontext::new(&mrb, c"frozen_reg.rb")
            .expect("allocating the context must succeed");

        // The handle still resolves once the class is frozen, but
        // registering onto it raises FrozenError — caught into Err the
        // same way the other definition rejections are.
        cxt.load_nstring(b"class BeniFrozenReg; end; BeniFrozenReg.freeze");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining and freezing must not raise"
        );
        let class = mrb
            .class_get(c"BeniFrozenReg")
            .expect("the class is defined");

        assert!(matches!(
            class.define_method(&mrb, c"m", crate::method!(answer_seven, 0)),
            Err(Error::Exception(_))
        ));
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
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");

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
        let got = receiver
            .funcall(&mrb, c"secret", &[])
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
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"answer", &[])
            .expect("the registered method must not raise");
        assert_eq!(unsafe { got.unbox_integer() }, 7);

        // Object trait: singleton registration on the class handle,
        // invoked through the reified class value.
        class
            .define_singleton_method(&mrb, c"class_answer", crate::method!(answer_nine, 0))
            .expect("registering the singleton method must succeed");
        // SAFETY: `class` is a live handle from this VM.
        let class_value = unsafe { class.to_value(&mrb) };
        let got = class_value
            .funcall(&mrb, c"class_answer", &[])
            .expect("the registered class method must not raise");
        assert_eq!(unsafe { got.unbox_integer() }, 9);

        // Lookup round-trip through the trait.
        let fetched = outer
            .class_get(&mrb, c"Widget")
            .expect("fetching the nested class must succeed");
        assert_eq!(fetched.name(&mrb), Some("BeniTrait::Widget"));
    }

    #[test]
    fn define_module_function_attaches_to_module_and_includers() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let module = mrb
            .define_module(c"BeniModFn")
            .expect("defining the module must succeed");
        module
            .define_module_function(&mrb, c"seven", crate::method!(answer_seven, 0))
            .expect("registering the module function must succeed");

        let cxt = crate::Ccontext::new(&mrb, c"modfn_test.rb")
            .expect("allocating the compile context must succeed");

        // Callable directly on the module object — the singleton form.
        let direct = cxt.load_nstring(b"BeniModFn.seven");
        assert!(
            mrb.pending_exc().is_nil(),
            "calling the module function must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(unsafe { direct.unbox_integer() }, 7);

        // Callable as a bare private helper inside a class that mixes the
        // module in — the private-instance form.
        let included = cxt.load_nstring(
            b"class BeniModUser; include BeniModFn; def go; seven; end; end; BeniModUser.new.go",
        );
        assert!(
            mrb.pending_exc().is_nil(),
            "calling the mixed-in private form must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(unsafe { included.unbox_integer() }, 7);
    }

    #[test]
    fn module_function_instance_form_is_private() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let module = mrb
            .define_module(c"BeniModFnPriv")
            .expect("defining the module must succeed");
        module
            .define_module_function(&mrb, c"seven", crate::method!(answer_seven, 0))
            .expect("registering the module function must succeed");

        let cxt = crate::Ccontext::new(&mrb, c"modfn_priv_test.rb")
            .expect("allocating the compile context must succeed");
        cxt.load_nstring(b"class BeniModUserPriv; include BeniModFnPriv; end");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the includer must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // The mixed-in instance form is private: dispatching it with an
        // explicit receiver raises NoMethodError — the visibility half of
        // `module_function` the bare-helper call alone cannot prove.
        let _ = cxt.load_nstring(b"BeniModUserPriv.new.seven");
        let exc = mrb.pending_exc();
        assert!(
            !exc.is_nil(),
            "explicit-receiver dispatch of the private instance form must raise"
        );
        let message = Error::Exception(exc).message(&mrb);
        assert!(
            message.contains("private"),
            "the NoMethodError must name the visibility: {message}"
        );
        mrb.clear_exc();
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
    fn alias_method_binds_a_second_name_for_an_existing_method() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        let class = mrb
            .define_class(c"BeniAlias", object)
            .expect("defining the class must succeed");
        class
            .define_method(&mrb, c"answer", crate::method!(answer_seven, 0))
            .expect("registering the original method must succeed");

        class
            .alias_method(&mrb, c"original_answer", c"answer")
            .expect("aliasing an existing method must succeed");

        // The alias resolves to the same body as the original — the
        // consumer-visible point of preserving a method before override.
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"original_answer", &[])
            .expect("the aliased method must not raise");
        assert_eq!(unsafe { got.unbox_integer() }, 7);
    }

    #[test]
    fn include_module_mixes_in_and_rejects_a_cyclic_include() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        let helper = mrb
            .define_module(c"BeniMixin")
            .expect("defining the module must succeed");
        helper
            .define_method(&mrb, c"helped", crate::method!(answer_seven, 0))
            .expect("registering the module method must succeed");

        let class = mrb
            .define_class(c"BeniHost", object)
            .expect("defining the class must succeed");
        class
            .include_module(&mrb, helper)
            .expect("including the module must succeed");

        // An instance of the host now answers the mixed-in method.
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"helped", &[])
            .expect("the mixed-in method must not raise");
        assert_eq!(unsafe { got.unbox_integer() }, 7);

        // Including a module into itself is a cyclic include — rejected.
        assert!(helper.include_module(&mrb, helper).is_err());
    }

    #[test]
    fn alias_method_surfaces_name_error_for_missing_original() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let object = mrb.object_class();

        let class = mrb
            .define_class(c"BeniAliasMissing", object)
            .expect("defining the class must succeed");

        // mruby raises NameError when the aliased method does not exist
        // (vendored src/class.c mrb_method_search) — it must surface as
        // Err, not a longjmp.
        let err = class
            .alias_method(&mrb, c"shadow", c"no_such_method")
            .expect_err("aliasing a missing method must surface as Err");
        assert!(matches!(err, Error::Exception(_)));
        assert!(
            err.message(&mrb).contains("no_such_method"),
            "the NameError must name the missing method: {}",
            err.message(&mrb)
        );
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
