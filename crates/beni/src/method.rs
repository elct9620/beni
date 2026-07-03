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
//!      format repeated per arity) — for a fixed-arity (`n>0`) cfunc
//!      this is also the argument-count enforcement point: a
//!      mismatched count raises `ArgumentError` (via a longjmp that
//!      crosses the bridge's `catch_unwind`) before any `FromValue`
//!      conversion runs,
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
/// from the arity: `-1` is any, `arity` is the required positional
/// count, and `opt` the optional positional count that follows them.
/// `block` declares the method accepts a block, which the bridge
/// reads into an `Option<Proc>` trailing parameter.
#[derive(Copy, Clone)]
pub struct MethodDef {
    pub(crate) func: crate::mrb_func_t,
    pub(crate) arity: i8,
    pub(crate) opt: i8,
    pub(crate) block: bool,
}

impl MethodDef {
    /// Plumbing constructor for the `method!` macro — the macro
    /// expands in the consumer's crate, so this must be reachable,
    /// but registrations should always go through the macro.
    #[doc(hidden)]
    pub const fn new(func: crate::mrb_func_t, arity: i8) -> Self {
        Self {
            func,
            arity,
            opt: 0,
            block: false,
        }
    }

    /// As `new`, declaring `opt` optional positionals after the
    /// required ones — the constructor the `method!(f, req, opt)` form
    /// expands to.
    #[doc(hidden)]
    pub const fn new_with_opt(func: crate::mrb_func_t, arity: i8, opt: i8) -> Self {
        Self {
            func,
            arity,
            opt,
            block: false,
        }
    }

    /// As `new`, declaring the method accepts a block after its
    /// required positionals — the constructor the `method!(f, req, &)`
    /// form expands to.
    #[doc(hidden)]
    pub const fn new_with_block(func: crate::mrb_func_t, arity: i8) -> Self {
        Self {
            func,
            arity,
            opt: 0,
            block: true,
        }
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
pub(crate) fn core_exception(mrb: &Mrb, class_name: &core::ffi::CStr, msg: &str) -> Value {
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

/// Generate one `MethodReqOpt` trait: the typed crossing for a
/// registered function with `$req` required positionals followed by
/// `$opt` optional ones. The optional parameters are `Option<O>` on
/// the wrapped function — `Some` when the caller supplied the slot,
/// `None` when it was omitted. The format string separates the two
/// groups with `|`, mruby's optional marker.
///
/// An omitted optional leaves its out-parameter untouched, so each
/// optional slot is seeded with the undef sentinel and read back: an
/// unchanged (still-undef) slot is the omitted case, any other value
/// the supplied case converted through `FromValue`.
macro_rules! define_method_req_opt_trait {
    (
        $(#[$attr:meta])* $name:ident, $fmt:literal,
        [$(($req:ident, $rt:ident)),*],
        [$(($opt:ident, $ot:ident)),*]
    ) => {
        $(#[$attr])*
        pub trait $name<$($rt,)* $($ot,)* Res>
        where
            Self: Sized + Fn(&Mrb, Value $(, $rt)* $(, Option<$ot>)*) -> Res,
            $($rt: FromValue,)*
            $($ot: FromValue,)*
            Res: MethodReturn,
        {
            /// Read and convert the call-frame arguments, run the
            /// wrapped function, and project its return. A failed
            /// argument conversion — required or supplied optional —
            /// returns `Err` before the wrapped function runs.
            #[doc(hidden)]
            fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                #[cfg(mruby_linked)]
                {
                    $(let mut $req = sys::mrb_value::zeroed();)*
                    // SAFETY: pure value computation; the undef sentinel
                    // marks an optional slot mruby leaves untouched.
                    $(let mut $opt = unsafe { sys::mrb_undef_value_func() };)*
                    // SAFETY: `mrb` is alive; each out-parameter is a
                    // valid `*mut mrb_value`; the format string holds
                    // one `o` per out-parameter, `|` before the
                    // optional group.
                    unsafe {
                        sys::mrb_get_args(
                            mrb.as_ptr(),
                            $fmt.as_ptr()
                            $(, &mut $req as *mut sys::mrb_value)*
                            $(, &mut $opt as *mut sys::mrb_value)*
                        );
                    }
                    $(
                        let $req = $rt::from_value(Value::from_raw($req))
                            .ok_or_else(|| arg_type_error::<$rt>(mrb))?;
                    )*
                    $(
                        // SAFETY: `mrb` is alive; `$opt` is a valid value.
                        let $opt = if unsafe { sys::mrb_undef_p_func($opt) } {
                            None
                        } else {
                            Some(
                                $ot::from_value(Value::from_raw($opt))
                                    .ok_or_else(|| arg_type_error::<$ot>(mrb))?,
                            )
                        };
                    )*
                    (self)(mrb, self_ $(, $req)* $(, $opt)*).into_method_return(mrb)
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

        impl<Func, $($rt,)* $($ot,)* Res> $name<$($rt,)* $($ot,)* Res> for Func
        where
            Func: Fn(&Mrb, Value $(, $rt)* $(, Option<$ot>)*) -> Res,
            $($rt: FromValue,)*
            $($ot: FromValue,)*
            Res: MethodReturn,
        {
        }
    };
}

define_method_req_opt_trait!(
    /// Typed crossing for a method with one optional positional and
    /// no required ones.
    Method0Opt1,
    c"|o",
    [],
    [(a, O0)]
);
define_method_req_opt_trait!(
    /// Typed crossing for a method with one required positional
    /// followed by one optional.
    Method1Opt1,
    c"o|o",
    [(a, T0)],
    [(b, O0)]
);

/// Generate one `MethodReqBlock` trait: the typed crossing for a
/// registered function with `$req` required positionals followed by a
/// block parameter. The block is an `Option<Proc>` trailing parameter
/// on the wrapped function — `Some` when the caller passed a block,
/// `None` when none was passed. The format string ends in `&`,
/// mruby's block marker, which reads the call's block slot.
///
/// mruby leaves the block slot nil when no block is passed, so a nil
/// slot is the `None` case; any other value is a `Proc` the slot is
/// guaranteed to carry, wrapped through the unchecked downcast.
macro_rules! define_method_req_block_trait {
    (
        $(#[$attr:meta])* $name:ident, $fmt:literal,
        [$(($req:ident, $rt:ident)),*]
    ) => {
        $(#[$attr])*
        pub trait $name<$($rt,)* Res>
        where
            Self: Sized + Fn(&Mrb, Value $(, $rt)*, Option<crate::Proc>) -> Res,
            $($rt: FromValue,)*
            Res: MethodReturn,
        {
            /// Read and convert the call-frame arguments and the block,
            /// run the wrapped function, and project its return. A
            /// failed required-argument conversion returns `Err` before
            /// the wrapped function runs.
            #[doc(hidden)]
            fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                #[cfg(mruby_linked)]
                {
                    $(let mut $req = sys::mrb_value::zeroed();)*
                    let mut block = sys::mrb_value::zeroed();
                    // SAFETY: `mrb` is alive; each out-parameter is a
                    // valid `*mut mrb_value`; the format string holds
                    // one `o` per required out-parameter and a trailing
                    // `&` for the block slot.
                    unsafe {
                        sys::mrb_get_args(
                            mrb.as_ptr(),
                            $fmt.as_ptr()
                            $(, &mut $req as *mut sys::mrb_value)*,
                            &mut block as *mut sys::mrb_value,
                        );
                    }
                    $(
                        let $req = $rt::from_value(Value::from_raw($req))
                            .ok_or_else(|| arg_type_error::<$rt>(mrb))?;
                    )*
                    // SAFETY: `mrb` is alive; `block` is a valid value.
                    let block = if unsafe { sys::mrb_nil_p_func(block) } {
                        None
                    } else {
                        // The block slot carries a Proc whenever it is
                        // not nil, so the unchecked downcast is sound.
                        // SAFETY: the non-nil block slot is Proc-tagged
                        // by mruby's call convention.
                        Some(unsafe { crate::Proc::from_value_unchecked(Value::from_raw(block)) })
                    };
                    (self)(mrb, self_ $(, $req)*, block).into_method_return(mrb)
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

        impl<Func, $($rt,)* Res> $name<$($rt,)* Res> for Func
        where
            Func: Fn(&Mrb, Value $(, $rt)*, Option<crate::Proc>) -> Res,
            $($rt: FromValue,)*
            Res: MethodReturn,
        {
        }
    };
}

define_method_req_block_trait!(
    /// Typed crossing for a method that accepts a block and no
    /// required positionals.
    Method0Block,
    c"&",
    []
);
define_method_req_block_trait!(
    /// Typed crossing for a method with one required positional and a
    /// block.
    Method1Block,
    c"o&",
    [(a, T0)]
);
define_method_req_block_trait!(
    /// Typed crossing for a method with two required positionals and a
    /// block.
    Method2Block,
    c"oo&",
    [(a, T0), (b, T1)]
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
/// The arity follows the function: `0..=4` for that many required
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
///
/// A second count declares optional positionals after the required
/// ones; each optional is an `Option` trailing parameter — `Some` when
/// supplied, `None` when omitted.
///
/// ```ignore
/// fn add(_mrb: &Mrb, _self: Value, a: i32, b: Option<i32>) -> i32 {
///     a + b.unwrap_or(0)
/// }
/// class.define_method(&mrb, c"add", method!(add, 1, 1))?;
/// ```
///
/// A trailing `&` declares the method accepts a block, read into a
/// final `Option<Proc>` parameter — `Some` when the caller passed a
/// block, `None` otherwise — that the body invokes through
/// `Proc::call`.
///
/// ```ignore
/// fn each(mrb: &Mrb, _self: Value, a: i32, block: Option<Proc>) -> Result<Value, Error> {
///     match block {
///         Some(b) => b.call(mrb, &[a.into_value(mrb)]),
///         None => Ok(Value::nil()),
///     }
/// }
/// class.define_method(&mrb, c"each", method!(each, 1, &))?;
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
    ($f:expr, 0, 1) => {
        $crate::__method_req_opt!($f, 0, 1, Method0Opt1)
    };
    ($f:expr, 1, 1) => {
        $crate::__method_req_opt!($f, 1, 1, Method1Opt1)
    };
    ($f:expr, 0, &) => {
        $crate::__method_block!($f, 0, Method0Block)
    };
    ($f:expr, 1, &) => {
        $crate::__method_block!($f, 1, Method1Block)
    };
    ($f:expr, 2, &) => {
        $crate::__method_block!($f, 2, Method2Block)
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

/// Shared expansion behind `method!`'s required-plus-optional arities.
/// As `__method_arity!`, but carries the optional count so the aspec
/// derives the required-and-optional form. Not part of the public
/// surface — `#[macro_export]` is only required so the `method!`
/// expansion can reach it from consumer crates.
#[doc(hidden)]
#[macro_export]
macro_rules! __method_req_opt {
    ($f:expr, $req:literal, $opt:literal, $trait_:ident) => {{
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
            // names.
            unsafe { $crate::method::$trait_::call_handle_error(f, mrb, self_) }
        }
        $crate::method::MethodDef::new_with_opt(bridge, $req, $opt)
    }};
}

/// Shared expansion behind `method!`'s block-accepting arities. As
/// `__method_arity!`, but marks the def block-accepting so the aspec
/// ORs in the block flag. Not part of the public surface —
/// `#[macro_export]` is only required so the `method!` expansion can
/// reach it from consumer crates.
#[doc(hidden)]
#[macro_export]
macro_rules! __method_block {
    ($f:expr, $req:literal, $trait_:ident) => {{
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
            // names.
            unsafe { $crate::method::$trait_::call_handle_error(f, mrb, self_) }
        }
        $crate::method::MethodDef::new_with_block(bridge, $req)
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

    // Optional trailing argument: present binds `Some`, omitted binds
    // `None` (defaulting to a base of 0).
    fn opt_add(_mrb: &Mrb, _self: Value, a: i32, b: Option<i32>) -> i32 {
        a + b.unwrap_or(0)
    }

    // No required arguments, one optional — proves the all-optional
    // form reads the lone optional slot.
    fn opt_only(_mrb: &Mrb, _self: Value, a: Option<i32>) -> i32 {
        a.unwrap_or(-1)
    }

    // Block-accepting method: yields the required argument to the block
    // when one was passed, returning the block's value; with no block
    // the slot binds `None` and the body returns the argument unchanged.
    fn apply_block(
        mrb: &Mrb,
        _self: Value,
        a: i32,
        block: Option<crate::Proc>,
    ) -> Result<Value, Error> {
        match block {
            Some(b) => b.call(mrb, &[a.into_value(mrb)]),
            None => Ok(a.into_value(mrb)),
        }
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

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let args = [Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)];
        let got = receiver
            .funcall(&mrb, c"add", &args)
            .expect("the call must not raise");
        assert_eq!(i32::from_value(got), Some(3));
    }

    #[test]
    fn fixed_arity_raises_argument_error_on_wrong_count() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniArityAdder");
        class
            .define_method(&mrb, c"add", method!(add, 2))
            .expect("registering the fixed-arity method must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");

        // `mrb_get_args` enforces the `MRB_ARGS_REQ(2)` count before any
        // `FromValue` conversion runs, raising `ArgumentError` whose
        // longjmp crosses the bridge's `catch_unwind` — both too few and
        // too many positionals take that path.
        for wrong in [
            vec![Value::from_int(&mrb, 1)],
            vec![
                Value::from_int(&mrb, 1),
                Value::from_int(&mrb, 2),
                Value::from_int(&mrb, 3),
            ],
        ] {
            let err = receiver
                .funcall(&mrb, c"add", &wrong)
                .expect_err("a wrong argument count must surface as Err");
            match err {
                Error::Exception(exc) => assert_eq!(exc.classname(&mrb), "ArgumentError"),
                Error::Panic(_) => panic!("a wrong argument count must raise, not panic"),
            }
        }

        // The count-error longjmp left the VM intact: a correctly-counted
        // call still dispatches and returns.
        let got = receiver
            .funcall(
                &mrb,
                c"add",
                &[Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)],
            )
            .expect("the VM survives the count-error longjmp and the next call runs");
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
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
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
    fn optional_argument_defaults_to_none_when_omitted() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniOptAdder");
        class
            .define_method(&mrb, c"add", method!(opt_add, 1, 1))
            .expect("registering the optional-arg method must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");

        // Omitting the optional argument binds `None`: the body sees
        // the default base of 0.
        let omitted = receiver
            .funcall(&mrb, c"add", &[Value::from_int(&mrb, 7)])
            .expect("the call with the optional omitted must not raise");
        assert_eq!(i32::from_value(omitted), Some(7));

        // Supplying it binds `Some`.
        let supplied = receiver
            .funcall(
                &mrb,
                c"add",
                &[Value::from_int(&mrb, 7), Value::from_int(&mrb, 5)],
            )
            .expect("the call with the optional supplied must not raise");
        assert_eq!(i32::from_value(supplied), Some(12));
    }

    #[test]
    fn all_optional_method_reads_its_lone_slot() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniOptOnly");
        class
            .define_method(&mrb, c"v", method!(opt_only, 0, 1))
            .expect("registering the all-optional method must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");

        let omitted = receiver
            .funcall(&mrb, c"v", &[])
            .expect("the call with no arguments must not raise");
        assert_eq!(i32::from_value(omitted), Some(-1));

        let supplied = receiver
            .funcall(&mrb, c"v", &[Value::from_int(&mrb, 42)])
            .expect("the call with the optional supplied must not raise");
        assert_eq!(i32::from_value(supplied), Some(42));
    }

    #[test]
    fn supplied_optional_failing_from_value_raises() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniOptStrict");
        class
            .define_method(&mrb, c"add", method!(opt_add, 1, 1))
            .expect("registering the optional-arg method must succeed");

        // A supplied optional that fails its i32 conversion raises, just
        // as a required argument does — an omitted slot would bind None
        // instead, so the raise is specific to the supplied case.
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let err = receiver
            .funcall(
                &mrb,
                c"add",
                &[Value::from_int(&mrb, 1), Value::from_float(&mrb, 1.5)],
            )
            .expect_err("the supplied optional's conversion failure must raise");
        assert!(
            err.message(&mrb).contains("i32"),
            "the TypeError must name the expected Rust type: {}",
            err.message(&mrb)
        );
    }

    #[test]
    fn block_accepting_method_yields_to_a_passed_block() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniBlockApply");
        class
            .define_method(&mrb, c"apply", method!(apply_block, 1, &))
            .expect("registering the block-accepting method must succeed");

        // Calling with a block: the registered method receives it as
        // `Some(Proc)` and yields the argument to it, here doubling it.
        let cxt = crate::Ccontext::new(&mrb, c"block_method_test.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"BeniBlockApply.new.apply(21) { |x| x * 2 }");
        assert!(
            mrb.pending_exc().is_nil(),
            "the block-yielding call must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(i32::from_value(got), Some(42));
    }

    #[test]
    fn block_accepting_method_binds_none_without_a_block() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = fresh_class(&mrb, c"BeniBlockOptional");
        class
            .define_method(&mrb, c"apply", method!(apply_block, 1, &))
            .expect("registering the block-accepting method must succeed");

        // No block passed: the slot is nil, the parameter binds `None`,
        // and the body returns the argument unchanged.
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"apply", &[Value::from_int(&mrb, 7)])
            .expect("the call without a block must not raise");
        assert_eq!(i32::from_value(got), Some(7));
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
        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
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

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
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
