//! Exposing Rust functions as mruby methods — beni's mirror of
//! `magnus::method`.
//!
//! ## Mechanism
//!
//! mruby's `mrb_define_method` takes a bare C function pointer with
//! no userdata slot, so a registered Rust function must be reachable
//! from a monomorphic `extern "C"` bridge. The `method!` macro
//! expands one anonymous bridge per registration site; the bridge
//! delegates to the matching `MethodN` trait, which owns the typed
//! crossing:
//!
//!   1. read the call-frame arguments via `mrb_get_args` (the `"o"`
//!      format repeated per arity),
//!   2. convert each through `FromValue` — a failed conversion
//!      raises `TypeError` to the Ruby caller **before** the wrapped
//!      function runs,
//!   3. call the function inside `catch_unwind` — a Rust panic is
//!      converted to a `RuntimeError` raised to the Ruby caller
//!      instead of unwinding into mruby's C frames,
//!   4. convert the return value through `MethodReturn`
//!      (`IntoValue`, or `Result<IntoValue, Error>` for fallible
//!      bodies, whose `Err` raises to the Ruby caller).
//!
//! Unlike CRuby, mruby does not split the argv by arity at the C
//! signature — every bridge is `(mrb_state*, mrb_value) ->
//! mrb_value` — so the arity cannot be recovered from the bridge's
//! type. `method!` therefore yields a `MethodDef` carrying the
//! bridge pointer and the arity, and `Module::define_method` derives
//! the mruby aspec from it.
//!
//! Like `magnus::method!`, the macro accepts function items and
//! non-capturing closures; a capturing closure fails to compile
//! because the expansion nests it inside an `extern "C" fn`.

use crate::{Error, FromValue, IntoValue, Mrb, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

#[cfg(mruby_linked)]
use crate::error::panic_message;

/// Bridge + arity pair produced by the `method!` macro and
/// consumed by `Module::define_method` /
/// `Object::define_singleton_method`, which derives the mruby aspec
/// from the arity (`-1` = any, `0..` = that many required
/// positionals).
#[derive(Copy, Clone)]
pub struct MethodDef {
    pub(crate) func: crate::mrb_func_t,
    pub(crate) arity: i8,
}

impl MethodDef {
    /// Plumbing constructor for the `method!` macro — the macro
    /// expands in the consumer's crate, so this must be reachable,
    /// but registrations should always go through the macro.
    #[doc(hidden)]
    pub const fn new(func: crate::mrb_func_t, arity: i8) -> Self {
        Self { func, arity }
    }
}

/// Return seam for registered methods — beni's mirror of magnus's
/// `ReturnValue`. Implemented for every `IntoValue` type (infallible
/// bodies) and for `Result<IntoValue, Error>` (fallible bodies,
/// whose `Err` is raised to the Ruby caller).
pub trait MethodReturn {
    /// Project the body's return into the value domain, or the error
    /// the bridge raises to the Ruby caller.
    fn into_method_return(self, mrb: &Mrb) -> Result<Value, Error>;
}

impl<T> MethodReturn for Result<T, Error>
where
    T: IntoValue,
{
    #[inline]
    fn into_method_return(self, mrb: &Mrb) -> Result<Value, Error> {
        self.map(|val| val.into_value(mrb))
    }
}

impl<T> MethodReturn for T
where
    T: IntoValue,
{
    #[inline]
    fn into_method_return(self, mrb: &Mrb) -> Result<Value, Error> {
        Ok(self.into_value(mrb))
    }
}

/// Build an exception of the named core class carrying `msg`'s
/// bytes, copied into the VM. The lookup cannot miss for core
/// classes (`TypeError`, `RuntimeError`).
#[cfg(mruby_linked)]
fn core_exception(mrb: &Mrb, class_name: &core::ffi::CStr, msg: &str) -> Value {
    // SAFETY: `mrb` is alive; `class_name` is NUL-terminated and
    // names a core class present in every VM.
    let class =
        crate::RClass::from_raw(unsafe { sys::mrb_class_get(mrb.as_ptr(), class_name.as_ptr()) });
    class.exc_new(mrb, msg)
}

/// The `TypeError` a bridge raises when an argument fails its
/// `FromValue` conversion — named after the Rust type the registered
/// function expected.
#[cfg(mruby_linked)]
fn arg_type_error<T>(mrb: &Mrb) -> Error {
    let msg = format!(
        "wrong argument type (expected {})",
        core::any::type_name::<T>()
    );
    Error::Exception(core_exception(mrb, c"TypeError", &msg))
}

/// Convert `err` into a pending mruby exception and long-jump to the
/// Ruby caller. A `Panic` is wrapped as a `RuntimeError`; its
/// message `String` is dropped before the raise because the
/// long-jump runs no Rust drops.
///
/// # Safety
///
/// Only callable from a bridge frame mruby may unwind out of, with
/// no live Rust values needing `Drop` on the caller's frame.
#[cfg(mruby_linked)]
unsafe fn raise_error(mrb: &Mrb, err: Error) -> ! {
    let exc = match err {
        Error::Exception(exc) => exc,
        Error::Panic(msg) => {
            let exc = core_exception(mrb, c"RuntimeError", &msg);
            drop(msg);
            exc
        }
    };
    // SAFETY: bridge frame — forwarded from the caller. bindgen
    // drops the `mrb_noreturn` attribute, so the diverging signature
    // is restated via `unreachable_unchecked` (`mrb_exc_raise`
    // long-jumps before control can reach it).
    unsafe { sys::mrb_exc_raise(mrb.as_ptr(), exc.into_raw()) };
    unsafe { core::hint::unreachable_unchecked() }
}

/// Wrap the conversion + body pipeline in the panic boundary and
/// route any error into a raise — the shared tail of every
/// `call_handle_error` below.
///
/// # Safety
///
/// As `raise_error`: bridge frame only.
#[cfg(mruby_linked)]
unsafe fn handle_error<F>(mrb: &Mrb, f: F) -> Value
where
    F: FnOnce() -> Result<Value, Error>,
{
    let res = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(res) => res,
        Err(payload) => Err(Error::Panic(panic_message(payload))),
    };
    match res {
        Ok(value) => value,
        // SAFETY: forwarded from the caller's bridge-frame contract.
        Err(err) => unsafe { raise_error(mrb, err) },
    }
}

/// Generate one `MethodN` trait: the typed crossing for a registered
/// function of fixed arity. `call_convert_value` owns steps 1–2 and
/// 4 of the module-doc pipeline; `call_handle_error` adds the panic
/// boundary and the raise.
macro_rules! define_method_trait {
    ($(#[$attr:meta])* $name:ident, $fmt:literal, $(($arg:ident, $t:ident)),*) => {
        $(#[$attr])*
        pub trait $name<$($t,)* Res>
        where
            Self: Sized + Fn(&Mrb, Value $(, $t)*) -> Res,
            $($t: FromValue,)*
            Res: MethodReturn,
        {
            /// Read and convert the call-frame arguments, run the
            /// wrapped function, and project its return. A failed
            /// argument conversion returns `Err` before the wrapped
            /// function runs.
            #[doc(hidden)]
            fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                #[cfg(mruby_linked)]
                {
                    $(let mut $arg = sys::mrb_value::zeroed();)*
                    // SAFETY: `mrb` is alive; each out-parameter is a
                    // valid `*mut mrb_value`; the format string holds
                    // one `o` per out-parameter.
                    unsafe {
                        sys::mrb_get_args(
                            mrb.as_ptr(),
                            $fmt.as_ptr()
                            $(, &mut $arg as *mut sys::mrb_value)*
                        );
                    }
                    $(
                        let $arg = $t::from_value(Value::from_raw($arg))
                            .ok_or_else(|| arg_type_error::<$t>(mrb))?;
                    )*
                    (self)(mrb, self_ $(, $arg)*).into_method_return(mrb)
                }
                #[cfg(not(mruby_linked))]
                {
                    let _ = (mrb, self_);
                    crate::not_linked()
                }
            }

            /// Bridge entry: `call_convert_value` inside the panic
            /// boundary, raising any error to the Ruby caller.
            ///
            /// # Safety
            ///
            /// Bridge frame only — the raise long-jumps out.
            #[doc(hidden)]
            unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
                #[cfg(mruby_linked)]
                {
                    // SAFETY: forwarded from the caller.
                    unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
                }
                #[cfg(not(mruby_linked))]
                {
                    let _ = (mrb, self_);
                    crate::not_linked()
                }
            }
        }

        impl<Func, $($t,)* Res> $name<$($t,)* Res> for Func
        where
            Func: Fn(&Mrb, Value $(, $t)*) -> Res,
            $($t: FromValue,)*
            Res: MethodReturn,
        {
        }
    };
}

define_method_trait!(
    /// Typed crossing for a zero-argument method
    /// (`Fn(&Mrb, Value) -> Res`).
    Method0,
    c"",
);
define_method_trait!(
    /// Typed crossing for a one-argument method.
    Method1,
    c"o",
    (a, T0)
);
define_method_trait!(
    /// Typed crossing for a two-argument method.
    Method2,
    c"oo",
    (a, T0),
    (b, T1)
);
define_method_trait!(
    /// Typed crossing for a three-argument method.
    Method3,
    c"ooo",
    (a, T0),
    (b, T1),
    (c, T2)
);
define_method_trait!(
    /// Typed crossing for a four-argument method.
    Method4,
    c"oooo",
    (a, T0),
    (b, T1),
    (c, T2),
    (d, T3)
);

/// Typed crossing for an any-arity method (`method!(f, -1)`): the
/// wrapped function reads the call frame itself via
/// `Mrb::get_args` (`format::Rest` and friends), and registration
/// uses the any-arguments aspec. The panic boundary and return seam
/// still apply.
pub trait MethodAny<Res>
where
    Self: Sized + Fn(&Mrb, Value) -> Res,
    Res: MethodReturn,
{
    /// Run the wrapped function and project its return.
    #[doc(hidden)]
    fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
        (self)(mrb, self_).into_method_return(mrb)
    }

    /// Bridge entry: `call_convert_value` inside the panic boundary,
    /// raising any error to the Ruby caller.
    ///
    /// # Safety
    ///
    /// Bridge frame only — the raise long-jumps out.
    #[doc(hidden)]
    unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from the caller.
            unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, self_);
            crate::not_linked()
        }
    }
}

impl<Func, Res> MethodAny<Res> for Func
where
    Func: Fn(&Mrb, Value) -> Res,
    Res: MethodReturn,
{
}

/// Wrap a Rust function as an mruby method registration.
///
/// The second argument is the arity: `0..=4` for that many required
/// positional arguments (each converted through `FromValue` before
/// the function runs), or `-1` for a function that reads the call
/// frame itself via `Mrb::get_args`.
///
/// ```ignore
/// fn add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
///     a + b
/// }
/// class.define_method(&mrb, c"add", method!(add, 2))?;
/// ```
#[macro_export]
macro_rules! method {
    ($f:expr, -1) => {{
        unsafe extern "C" fn bridge(
            mrb: *mut $crate::sys::mrb_state,
            self_: $crate::Value,
        ) -> $crate::Value {
            // Evaluated outside the unsafe block so caller-supplied
            // code is never silently wrapped in it.
            let f = $f;
            // SAFETY: mruby invokes the bridge with a live state
            // pointer that outlives the call frame.
            let mrb = unsafe { $crate::Mrb::borrow_raw(&mrb) };
            // SAFETY: this is the bridge frame the raise contract
            // names. The explicit trait path disambiguates from the
            // fixed-arity traits, whose zero-argument shape shares
            // this signature.
            unsafe { $crate::method::MethodAny::call_handle_error(f, mrb, self_) }
        }
        $crate::method::MethodDef::new(bridge, -1)
    }};
    ($f:expr, 0) => {
        $crate::__method_arity!($f, 0, Method0)
    };
    ($f:expr, 1) => {
        $crate::__method_arity!($f, 1, Method1)
    };
    ($f:expr, 2) => {
        $crate::__method_arity!($f, 2, Method2)
    };
    ($f:expr, 3) => {
        $crate::__method_arity!($f, 3, Method3)
    };
    ($f:expr, 4) => {
        $crate::__method_arity!($f, 4, Method4)
    };
}

/// Shared expansion behind `method!`'s fixed arities. Not part of
/// the public surface — `#[macro_export]` is only required so the
/// `method!` expansion can reach it from consumer crates.
#[doc(hidden)]
#[macro_export]
macro_rules! __method_arity {
    ($f:expr, $arity:literal, $trait_:ident) => {{
        unsafe extern "C" fn bridge(
            mrb: *mut $crate::sys::mrb_state,
            self_: $crate::Value,
        ) -> $crate::Value {
            // Evaluated outside the unsafe block so caller-supplied
            // code is never silently wrapped in it.
            let f = $f;
            // SAFETY: mruby invokes the bridge with a live state
            // pointer that outlives the call frame.
            let mrb = unsafe { $crate::Mrb::borrow_raw(&mrb) };
            // SAFETY: this is the bridge frame the raise contract
            // names. The explicit trait path disambiguates the
            // zero-argument shape from `MethodAny`, which shares its
            // signature.
            unsafe { $crate::method::$trait_::call_handle_error(f, mrb, self_) }
        }
        $crate::method::MethodDef::new(bridge, $arity)
    }};
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::Module;

    static TYPE_ERROR_BODY_RAN: AtomicBool = AtomicBool::new(false);

    fn add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
        a + b
    }

    fn observed_add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
        TYPE_ERROR_BODY_RAN.store(true, Ordering::SeqCst);
        a + b
    }

    fn boom(_mrb: &Mrb, _self: Value) -> Value {
        panic!("boom in registered method");
    }

    fn fallible(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
        Err(Error::Exception(core_exception(
            mrb,
            c"RuntimeError",
            "fallible body says no",
        )))
    }

    fn fresh_class(mrb: &Mrb, name: &core::ffi::CStr) -> crate::RClass {
        mrb.define_class(name, mrb.object_class())
            .expect("defining the test class must succeed")
    }

    #[test]
    fn typed_method_roundtrips_scalars() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniAdder");
        class
            .define_method(&mrb, c"add", method!(add, 2))
            .expect("registering the typed method must succeed");

        let receiver = class.obj_new(&mrb, &[]);
        let args = [Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)];
        let got = receiver
            .funcall(&mrb, c"add", &args)
            .expect("the call must not raise");
        assert_eq!(i32::from_value(got), Some(3));
    }

    #[test]
    fn from_value_failure_raises_before_body_runs() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniStrictAdder");
        class
            .define_method(&mrb, c"add", method!(observed_add, 2))
            .expect("registering the typed method must succeed");

        // A Float argument fails the i32 FromValue conversion: the
        // bridge must raise TypeError to the Ruby caller and the
        // wrapped function must never run.
        let receiver = class.obj_new(&mrb, &[]);
        let args = [Value::from_float(&mrb, 1.5), Value::from_int(&mrb, 2)];
        let err = receiver
            .funcall(&mrb, c"add", &args)
            .expect_err("the conversion failure must surface as a raise");
        assert!(
            err.message(&mrb).contains("i32"),
            "the TypeError must name the expected Rust type: {}",
            err.message(&mrb)
        );
        assert!(
            !TYPE_ERROR_BODY_RAN.load(Ordering::SeqCst),
            "the wrapped function must not run on conversion failure"
        );
    }

    #[test]
    fn panicking_method_surfaces_as_ruby_exception() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniPanicker");
        class
            .define_method(&mrb, c"detonate", method!(boom, 0))
            .expect("registering the panicking method must succeed");

        // The panic must be caught at the bridge and re-raised as a
        // RuntimeError the Ruby-side caller (here: the protect frame)
        // observes — never an unwind through mruby's C frames.
        let receiver = class.obj_new(&mrb, &[]);
        let err = receiver
            .funcall(&mrb, c"detonate", &[])
            .expect_err("the panic must surface as a Ruby exception");
        assert!(matches!(err, Error::Exception(_)));
        assert!(
            err.message(&mrb).contains("boom in registered method"),
            "the RuntimeError must carry the panic message: {}",
            err.message(&mrb)
        );
    }

    #[test]
    fn protect_surfaces_closure_panic_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .protect(|_| panic!("pop goes the closure"))
            .expect_err("the panic must surface as Err");
        match err {
            Error::Panic(msg) => assert!(msg.contains("pop goes the closure")),
            Error::Exception(_) => panic!("a closure panic must surface as Error::Panic"),
        }
    }

    #[test]
    fn result_returning_method_raises_its_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniFallible");
        class
            .define_method(&mrb, c"try", method!(fallible, 0))
            .expect("registering the fallible method must succeed");

        let receiver = class.obj_new(&mrb, &[]);
        let err = receiver
            .funcall(&mrb, c"try", &[])
            .expect_err("the body's Err must surface as a raise");
        assert!(
            err.message(&mrb).contains("fallible body says no"),
            "the raised exception must be the body's own: {}",
            err.message(&mrb)
        );
    }
}
