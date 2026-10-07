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
//!   1. read the call-frame arguments — straight from the argument
//!      vector when the call passed no keywords and a count the arity
//!      accepts, otherwise via `mrb_get_args` (the `"o"` format repeated
//!      per arity) under exception protection; a block-accepting arity
//!      reads once through a format that accepts every count and checks
//!      the count itself. A mismatch comes back as mruby's `ArgumentError`
//!      `Err` before any `TryConvert` conversion runs,
//!   2. convert the receiver, then each argument, through `TryConvert`
//!      — mirroring magnus's typed `self` — a failed conversion
//!      raises its exception to the Ruby caller **before** the wrapped
//!      function runs,
//!   3. call the function inside `catch_unwind` — a Rust panic is
//!      converted to a `RuntimeError` raised to the Ruby caller
//!      instead of unwinding into mruby's C frames,
//!   4. convert the return value through `ReturnValue`
//!      (`IntoValue`, or `Result<IntoValue, Error>` for fallible
//!      bodies, whose `Err` raises to the Ruby caller; a `FiberYield`
//!      suspends the running fiber as the bridge returns).
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

use crate::state::args::read_frame;
use crate::{sys::AsRawValue, Error, Mrb, TryConvert, Value};
use beni_sys as sys;

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

/// Return seam for registered methods — magnus's `ReturnValue`.
/// Implemented for every `IntoValue` type (infallible bodies) and for
/// `Result<IntoValue, Error>` (fallible bodies, whose `Err` is raised
/// to the Ruby caller), and with the `fiber` feature for `FiberYield`
/// and its `Result`. Sealed: the set of return kinds is closed.
///
/// ```compile_fail
/// struct Custom;
/// impl beni::method::private::ReturnValue for Custom {
///     fn into_return_value(
///         self,
///         _: &beni::Mrb,
///         _: beni::method::private::Bridge,
///     ) -> Result<beni::Value, beni::Error> {
///         unimplemented!()
///     }
/// }
/// ```
pub trait ReturnValue: private::ReturnValue {}

impl<T> ReturnValue for T where T: private::ReturnValue {}

/// Return seam for a Rust-defined proc's body — magnus's `BlockReturn`:
/// an `IntoValue` value, or a `Result` of one whose `Err` is raised to
/// the proc's caller. Sealed. A fiber yield is not among them: mruby
/// switches fibers only from a C method's return, and its proc calls pop
/// the call frame without the switch check (`vendor/mruby/src/vm.c`,
/// the `OP_SEND` proc-call path and `OP_BLKCALL`).
///
/// ```compile_fail
/// fn pause(mrb: &beni::Mrb) -> beni::Proc {
///     mrb.proc_from_fn(|mrb, args, _block| mrb.fiber_yield(args))
/// }
/// ```
pub trait BlockReturn: private::BlockReturn {}

impl<T> BlockReturn for T where T: private::BlockReturn {}

pub(crate) mod private {
    use crate::{Error, IntoValue, Mrb, Value};

    /// Proof that a registration's bridge is projecting its body's
    /// return. Only this crate makes one, so no caller outside a
    /// bridge can run a projection — a fiber yield's switches fibers,
    /// which mruby allows only as a method's return.
    pub struct Bridge(pub(super) ());

    /// The projection sealing `ReturnValue` to this crate.
    pub trait ReturnValue {
        /// Project the body's return into the value domain, or the
        /// error the bridge raises to the Ruby caller.
        fn into_return_value(self, mrb: &Mrb, bridge: Bridge) -> Result<Value, Error>;
    }

    impl<T> ReturnValue for Result<T, Error>
    where
        T: IntoValue,
    {
        #[inline]
        fn into_return_value(self, mrb: &Mrb, _: Bridge) -> Result<Value, Error> {
            self.map(|val| val.into_value(mrb))
        }
    }

    impl<T> ReturnValue for T
    where
        T: IntoValue,
    {
        #[inline]
        fn into_return_value(self, mrb: &Mrb, _: Bridge) -> Result<Value, Error> {
            Ok(self.into_value(mrb))
        }
    }

    /// The projection sealing `BlockReturn` to this crate.
    pub trait BlockReturn {
        /// Project the body's return into the value domain, or the
        /// error raised to the proc's caller.
        fn into_block_return(self, mrb: &Mrb) -> Result<Value, Error>;
    }

    impl<T> BlockReturn for Result<T, Error>
    where
        T: IntoValue,
    {
        #[inline]
        fn into_block_return(self, mrb: &Mrb) -> Result<Value, Error> {
            self.map(|val| val.into_value(mrb))
        }
    }

    impl<T> BlockReturn for T
    where
        T: IntoValue,
    {
        #[inline]
        fn into_block_return(self, mrb: &Mrb) -> Result<Value, Error> {
            Ok(self.into_value(mrb))
        }
    }
}

/// Build an exception of the named core exception class carrying
/// `msg`'s bytes, copied into the VM. The lookup cannot miss for core
/// exception classes (`TypeError`, `RuntimeError`).
pub(crate) fn core_exception(mrb: &Mrb, class_name: &core::ffi::CStr, msg: &str) -> Value {
    // SAFETY: `mrb` is alive; `class_name` is NUL-terminated and names a
    // core exception class present in every VM, so the pointer is an
    // exception class.
    let class = crate::ExceptionClass::from_raw_unchecked(unsafe {
        sys::mrb_class_get(mrb.as_ptr(), class_name.as_ptr())
    });
    class.exc_new(mrb, msg)
}

/// Convert `err` into a pending mruby exception and long-jump to the
/// Ruby caller. A `Syntax` is wrapped as a `SyntaxError` and a
/// `Panic` as a `RuntimeError`; each message `String` is dropped
/// before the raise, which leaves this frame without returning through
/// it.
///
/// The `SyntaxError` reads `line N: message`, the wording mruby's own
/// compiler produces, so a Ruby caller cannot tell whether the source
/// was compiled through beni or through mruby.
///
/// # Safety
///
/// Only callable from a bridge frame mruby may unwind out of, with
/// no live Rust values needing `Drop` on the caller's frame.
unsafe fn raise_error(mrb: &Mrb, err: Error) -> ! {
    let exc = match err {
        Error::Exception(exc) => exc,
        Error::Syntax(parse) => {
            let msg = format!("line {}: {}", parse.line(), parse.message());
            let exc = core_exception(mrb, c"SyntaxError", &msg);
            drop(msg);
            drop(parse);
            exc
        }
        Error::Panic(msg) => {
            let exc = core_exception(mrb, c"RuntimeError", &msg);
            drop(msg);
            exc
        }
    };
    // SAFETY: bridge frame — forwarded from the caller.
    // `mrb_exc_raise` is declared as never returning and the binding
    // carries that, so it satisfies the diverging signature.
    unsafe { sys::mrb_exc_raise(mrb.as_ptr(), exc.as_raw()) }
}

/// Wrap the conversion + body pipeline in the panic boundary and
/// route any error into a raise — the shared tail of every
/// `call_handle_error` below.
///
/// # Safety
///
/// As `raise_error`: bridge frame only.
pub(crate) unsafe fn handle_error<F>(mrb: &Mrb, f: F) -> Value
where
    F: FnOnce() -> Result<Value, Error>,
{
    match crate::sys::catch_unwind(std::panic::AssertUnwindSafe(f)).and_then(|res| res) {
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
        pub trait $name<S, $($t,)* Res>
        where
            Self: Sized + Fn(&Mrb, S $(, $t)*) -> Res,
            S: TryConvert,
            $($t: TryConvert,)*
            Res: ReturnValue,
        {
            /// Read and convert the call-frame arguments, run the
            /// wrapped function, and project its return. A failed
            /// receiver or argument conversion returns `Err` before the
            /// wrapped function runs.
            ///
            /// # Safety
            ///
            /// Bridge frame only — projecting a fiber yield switches
            /// fibers, which mruby allows only as the method's return.
            #[doc(hidden)]
            unsafe fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                let arity = <[&str]>::len(&[$(stringify!($arg)),*]);
                let args = match crate::state::args::frame_args(mrb, arity) {
                    Some(args) => args,
                    None => {
                        $(let mut $arg = sys::mrb_value::zeroed();)*
                        read_frame(mrb, |mrb| {
                            // SAFETY: `mrb` is alive; each out-parameter
                            // is a valid `*mut mrb_value`; the format
                            // string holds one `o` per out-parameter.
                            unsafe {
                                sys::mrb_get_args(
                                    mrb.as_ptr(),
                                    $fmt.as_ptr()
                                    $(, &mut $arg as *mut sys::mrb_value)*
                                );
                            }
                        })?;
                        [$(Value::from_raw_unchecked($arg)),*]
                    }
                };
                let [$($arg),*] = args;
                let self_ = S::try_convert(self_, mrb)?;
                $(
                    let $arg = $t::try_convert($arg, mrb)?;
                )*
                (self)(mrb, self_ $(, $arg)*).into_return_value(mrb, private::Bridge(()))
            }

            /// Bridge entry: `call_convert_value` inside the panic
            /// boundary, raising any error to the Ruby caller.
            ///
            /// # Safety
            ///
            /// Bridge frame only — the raise long-jumps out.
            #[doc(hidden)]
            unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
                // SAFETY: forwarded from the caller.
                unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
            }
        }

        impl<Func, S, $($t,)* Res> $name<S, $($t,)* Res> for Func
        where
            Func: Fn(&Mrb, S $(, $t)*) -> Res,
            S: TryConvert,
            $($t: TryConvert,)*
            Res: ReturnValue,
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
/// the supplied case converted through `TryConvert`.
macro_rules! define_method_req_opt_trait {
    (
        $(#[$attr:meta])* $name:ident, $fmt:literal,
        [$(($req:ident, $rt:ident)),*],
        [$(($opt:ident, $ot:ident)),*]
    ) => {
        $(#[$attr])*
        pub trait $name<S, $($rt,)* $($ot,)* Res>
        where
            Self: Sized + Fn(&Mrb, S $(, $rt)* $(, Option<$ot>)*) -> Res,
            S: TryConvert,
            $($rt: TryConvert,)*
            $($ot: TryConvert,)*
            Res: ReturnValue,
        {
            /// Read and convert the call-frame arguments, run the
            /// wrapped function, and project its return. A failed
            /// receiver or argument conversion — required or supplied
            /// optional — returns `Err` before the wrapped function runs.
            ///
            /// # Safety
            ///
            /// Bridge frame only — projecting a fiber yield switches
            /// fibers, which mruby allows only as the method's return.
            #[doc(hidden)]
            unsafe fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                let required = <[&str]>::len(&[$(stringify!($req)),*]);
                let args = match crate::state::args::frame_args(mrb, required) {
                    Some(args) => args,
                    None => {
                        $(let mut $req = sys::mrb_value::zeroed();)*
                        // SAFETY: pure value computation; the undef sentinel
                        // marks an optional slot mruby leaves untouched.
                        $(let mut $opt = unsafe { sys::mrb_undef_value_func() };)*
                        read_frame(mrb, |mrb| {
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
                        })?;
                        [$(Value::from_raw_unchecked($req),)* $(Value::from_raw_unchecked($opt)),*]
                    }
                };
                let [$($req,)* $($opt),*] = args;
                let self_ = S::try_convert(self_, mrb)?;
                $(
                    let $req = $rt::try_convert($req, mrb)?;
                )*
                $(
                    // SAFETY: `mrb` is alive; `$opt` is a valid value.
                    let $opt = if unsafe { sys::mrb_undef_p_func($opt.as_raw()) } {
                        None
                    } else {
                        Some($ot::try_convert($opt, mrb)?)
                    };
                )*
                (self)(mrb, self_ $(, $req)* $(, $opt)*).into_return_value(mrb, private::Bridge(()))
            }

            /// Bridge entry: `call_convert_value` inside the panic
            /// boundary, raising any error to the Ruby caller.
            ///
            /// # Safety
            ///
            /// Bridge frame only — the raise long-jumps out.
            #[doc(hidden)]
            unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
                // SAFETY: forwarded from the caller.
                unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
            }
        }

        impl<Func, S, $($rt,)* $($ot,)* Res> $name<S, $($rt,)* $($ot,)* Res> for Func
        where
            Func: Fn(&Mrb, S $(, $rt)* $(, Option<$ot>)*) -> Res,
            S: TryConvert,
            $($rt: TryConvert,)*
            $($ot: TryConvert,)*
            Res: ReturnValue,
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
/// `None` when none was passed. The frame is read once by
/// `frame_args_with_block`, which checks the count itself.
///
/// mruby leaves the block slot nil when no block is passed, so a nil
/// slot is the `None` case; any other value is a `Proc` the slot is
/// guaranteed to carry, wrapped through the unchecked downcast.
macro_rules! define_method_req_block_trait {
    (
        $(#[$attr:meta])* $name:ident,
        [$(($req:ident, $rt:ident)),*]
    ) => {
        $(#[$attr])*
        pub trait $name<S, $($rt,)* Res>
        where
            Self: Sized + Fn(&Mrb, S $(, $rt)*, Option<crate::Proc>) -> Res,
            S: TryConvert,
            $($rt: TryConvert,)*
            Res: ReturnValue,
        {
            /// Read and convert the call-frame arguments and the block,
            /// run the wrapped function, and project its return. A
            /// failed receiver or required-argument conversion returns
            /// `Err` before the wrapped function runs.
            ///
            /// # Safety
            ///
            /// Bridge frame only — projecting a fiber yield switches
            /// fibers, which mruby allows only as the method's return.
            #[doc(hidden)]
            unsafe fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
                let (args, block) = crate::state::args::frame_args_with_block(mrb)?;
                let [$($req),*] = args;
                let self_ = S::try_convert(self_, mrb)?;
                $(
                    let $req = $rt::try_convert($req, mrb)?;
                )*
                // SAFETY: `mrb` is alive; `block` is a valid value.
                let block = if unsafe { sys::mrb_nil_p_func(block.as_raw()) } {
                    None
                } else {
                    // The block slot carries a Proc whenever it is
                    // not nil, so the unchecked downcast is sound.
                    // SAFETY: the non-nil block slot is Proc-tagged
                    // by mruby's call convention.
                    Some(unsafe { crate::Proc::from_value_unchecked(block) })
                };
                (self)(mrb, self_ $(, $req)*, block).into_return_value(mrb, private::Bridge(()))
            }

            /// Bridge entry: `call_convert_value` inside the panic
            /// boundary, raising any error to the Ruby caller.
            ///
            /// # Safety
            ///
            /// Bridge frame only — the raise long-jumps out.
            #[doc(hidden)]
            unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
                // SAFETY: forwarded from the caller.
                unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
            }
        }

        impl<Func, S, $($rt,)* Res> $name<S, $($rt,)* Res> for Func
        where
            Func: Fn(&Mrb, S $(, $rt)*, Option<crate::Proc>) -> Res,
            S: TryConvert,
            $($rt: TryConvert,)*
            Res: ReturnValue,
        {
        }
    };
}

define_method_req_block_trait!(
    /// Typed crossing for a method that accepts a block and no
    /// required positionals.
    Method0Block,
    []
);
define_method_req_block_trait!(
    /// Typed crossing for a method with one required positional and a
    /// block.
    Method1Block,
    [(a, T0)]
);
define_method_req_block_trait!(
    /// Typed crossing for a method with two required positionals and a
    /// block.
    Method2Block,
    [(a, T0), (b, T1)]
);

/// Typed crossing for an any-arity method (`method!(f, -1)`): the
/// wrapped function receives the call's arguments as a slice — the
/// positionals, then a non-empty keyword hash — and registration uses
/// the any-arguments aspec. The panic boundary and return seam still
/// apply. Mirrors magnus's `method!(f, -1)`.
pub trait MethodAny<S, Res>
where
    Self: Sized + Fn(&Mrb, S, &[Value]) -> Res,
    S: TryConvert,
    Res: ReturnValue,
{
    /// Convert the receiver, run the wrapped function over the call's
    /// arguments, and project its return.
    ///
    /// # Safety
    ///
    /// Bridge frame only — projecting a fiber yield switches fibers,
    /// which mruby allows only as the method's return.
    #[doc(hidden)]
    unsafe fn call_convert_value(self, mrb: &Mrb, self_: Value) -> Result<Value, Error> {
        let rb_self = S::try_convert(self_, mrb)?;
        crate::state::args::with_args(mrb, |args| (self)(mrb, rb_self, args))
            .into_return_value(mrb, private::Bridge(()))
    }

    /// Bridge entry: `call_convert_value` inside the panic boundary,
    /// raising any error to the Ruby caller.
    ///
    /// # Safety
    ///
    /// Bridge frame only — the raise long-jumps out.
    #[doc(hidden)]
    unsafe fn call_handle_error(self, mrb: &Mrb, self_: Value) -> Value {
        // SAFETY: forwarded from the caller.
        unsafe { handle_error(mrb, || self.call_convert_value(mrb, self_)) }
    }
}

impl<Func, S, Res> MethodAny<S, Res> for Func
where
    Func: Fn(&Mrb, S, &[Value]) -> Res,
    S: TryConvert,
    Res: ReturnValue,
{
}

/// Wrap a Rust function as an mruby method registration.
///
/// The arity follows the function: `0..=4` for that many required
/// positional arguments (each converted through `TryConvert` before
/// the function runs), or `-1` for a function taking the call's
/// arguments as a trailing `&[Value]`, reading any further shape from
/// the frame via `scan_args::scan_args`. The receiver, the parameter
/// after `&Mrb`, converts through `TryConvert` too, so a method takes it
/// as the handle or Rust value it expects, as magnus's typed `self`.
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
///         None => Ok(crate::value::qnil().as_value()),
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
            // fixed-arity traits, which carry methods of the same name.
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
