//! Typed `Proc` newtype around a Proc-tagged `Value`.
//!
//! `Proc` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Proc` is exactly an `mrb_value` known to carry
//! an mruby `Proc` (a block). Construction is by checked `FromValue`
//! downcast or explicit unchecked cast from `Value`.
//!
//! Mirrors magnus's `block::Proc`: the protected `call` that yields to
//! the block lives here, beside the `Mrb` constructors that build a
//! `Proc` whose body is Rust.

use crate::method::BlockReturn;
use crate::try_convert::type_error;
use crate::{sys::AsRawValue, DataType, Error, FromValue, Mrb, ReprValue, TryConvert, Value};
use beni_sys as sys;
use core::cell::{Cell, UnsafeCell};

/// Typed handle on an mruby `Proc` (a block). `#[repr(transparent)]`
/// over `Value` so the C ABI is preserved.
///
/// Construct via the checked `FromValue` downcast (`Proc::from_value`,
/// tag-discriminated) or `Proc::from_value_unchecked` (assert that a
/// `Value` you already hold is Proc-tagged). Round-trip back to a
/// generic `Value` via `ReprValue::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Proc(pub(crate) Value);

impl Proc {
    /// Wrap a `Value` that the caller has already determined to be
    /// Proc-tagged (e.g. via the `FromValue` downcast or because it
    /// came straight off a block-holding slot).
    ///
    /// # Safety
    ///
    /// `v` must be Proc-tagged. Yielding through a non-Proc value is
    /// undefined per mruby's `mrb_yield_argv` contract.
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Yield to this block with `args` under exception protection.
    /// The block's normal return is `Ok(value)`; any non-local exit —
    /// a raised exception, or a `break` / `return` object the block
    /// throws — surfaces as `Err` instead of unwinding across FFI,
    /// mirroring `magnus::block::Proc::call`.
    ///
    /// Interpreting a non-local exit (a real `break` versus a `return`
    /// aimed past a frame versus a plain raise) is the caller's
    /// concern: `ReprValue::as_break` discriminates a break and reads its
    /// carried value, while the call-info frame indices that separate a
    /// break from a return-past-frame are VM internals reached through
    /// the unsafe `beni::sys` escape hatch.
    #[inline]
    pub fn call(self, mrb: &Mrb, args: &[Value]) -> Result<Value, Error> {
        let block_raw = self.0.as_raw();
        mrb.protect(|inner| {
            // `Value` is `#[repr(transparent)]` over `mrb_value`, so
            // the slice layout is mruby's argv exactly — the cast is
            // a no-op at codegen level.
            let argv = args.as_ptr() as *const sys::mrb_value;
            // SAFETY: `inner` is the live VM inside the protected
            // frame; `block_raw` is Proc-tagged by the
            // `from_value_unchecked` contract; every `args` entry
            // originates from the same VM and the slice outlives the
            // call.
            let raw = unsafe {
                sys::mrb_yield_argv(
                    inner.as_ptr(),
                    block_raw,
                    sys::mrb_int::try_from(args.len()).unwrap_or(sys::mrb_int::MAX),
                    argv,
                )
            };
            Value::from_raw_unchecked(raw)
        })
    }
}

impl Mrb {
    /// A `Proc` whose body is the plain function `block`, mirroring
    /// magnus's `Ruby::proc_new`. The body receives the call's
    /// arguments as an any-arity method does, and its block.
    pub fn proc_new<R>(&self, block: fn(&Mrb, &[Value], Option<Proc>) -> R) -> Proc
    where
        R: BlockReturn,
    {
        self.proc_with_body(Box::new(Shared {
            block: block as *const (),
            call: Shared::call_as::<R>,
        }))
    }

    /// A `Proc` whose body is the closure `block`, mirroring magnus's
    /// `Ruby::proc_from_fn`. The proc and every copy of it share the
    /// closure, which the interpreter drops once it has reclaimed them
    /// all. The collector never traces into the closure, so a value it
    /// captures stays valid only through a root. Calling the closure
    /// again while it runs raises `RuntimeError` to that caller.
    pub fn proc_from_fn<F, R>(&self, block: F) -> Proc
    where
        F: 'static + Send + FnMut(&Mrb, &[Value], Option<Proc>) -> R,
        R: BlockReturn,
    {
        self.proc_with_body(Box::new(Exclusive {
            running: Cell::new(false),
            body: UnsafeCell::new(block),
        }))
    }

    /// Hand `body` to a data carrier held in the one environment slot of
    /// a C-function proc, so the proc and its copies, which share that
    /// environment, keep the carrier reachable and the collector drops
    /// the body with the last of them.
    fn proc_with_body(&self, body: Box<dyn ProcBody>) -> Proc {
        let ptr = Box::into_raw(Box::new(body));
        // SAFETY: `self` is alive; `Object` accepts a data carrier
        // (`vendor/mruby/src/gc.c:579`), and `PROC_BODY` is `'static`
        // and frees exactly the box handed over here.
        let carrier = unsafe {
            sys::mrb_data_object_alloc(
                self.as_ptr(),
                self.object_class().as_internal(),
                ptr.cast(),
                PROC_BODY.as_raw(),
            )
        };
        // SAFETY: `carrier` is the live object just allocated.
        let env = [unsafe { sys::mrb_obj_value(carrier.cast()) }];
        // SAFETY: `self` is alive; the environment slot is copied from
        // `env`, which outlives the call; the proc answered is live.
        let proc_ = unsafe {
            sys::mrb_proc_new_cfunc_with_env(self.as_ptr(), proc_bridge, 1, env.as_ptr())
        };
        // SAFETY: `proc_` is the live Proc just allocated.
        unsafe {
            Proc::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_obj_value(proc_.cast())))
        }
    }
}

/// The data type a Rust-defined proc's body is carried under.
static PROC_BODY: DataType<Box<dyn ProcBody>> = DataType::new(c"rust proc body");

/// A Rust-defined proc's body, its return already projected.
trait ProcBody: Send {
    /// Run the body over the call's arguments and block.
    fn call(&self, mrb: &Mrb, args: &[Value], block: Option<Proc>) -> Result<Value, Error>;
}

/// A plain function, which re-entry cannot alias. Its return type lives
/// only in `call`, which reads `block` back at that type, so the carried
/// body is `'static` whatever the return type.
struct Shared {
    block: *const (),
    call: ErasedCall,
}

/// A plain function's call with its type erased to a pointer, run by
/// the `Shared::call_as` instance that knows the type.
type ErasedCall = unsafe fn(*const (), &Mrb, &[Value], Option<Proc>) -> Result<Value, Error>;

// SAFETY: `block` is a function pointer, which any thread may call.
unsafe impl Send for Shared {}

impl Shared {
    /// Run `block` as the function `proc_new` was handed.
    ///
    /// # Safety
    ///
    /// `block` came from a `fn(&Mrb, &[Value], Option<Proc>) -> R`.
    unsafe fn call_as<R: BlockReturn>(
        block: *const (),
        mrb: &Mrb,
        args: &[Value],
        proc_block: Option<Proc>,
    ) -> Result<Value, Error> {
        // SAFETY: the caller's contract names the function type.
        let block: fn(&Mrb, &[Value], Option<Proc>) -> R = unsafe { core::mem::transmute(block) };
        block(mrb, args, proc_block).into_block_return(mrb)
    }
}

impl ProcBody for Shared {
    fn call(&self, mrb: &Mrb, args: &[Value], block: Option<Proc>) -> Result<Value, Error> {
        // SAFETY: `proc_new` pairs `block` with `call_as` at its type.
        unsafe { (self.call)(self.block, mrb, args, block) }
    }
}

/// A closure, run by one call at a time.
struct Exclusive<F> {
    running: Cell<bool>,
    body: UnsafeCell<F>,
}

impl<F, R> ProcBody for Exclusive<F>
where
    F: Send + FnMut(&Mrb, &[Value], Option<Proc>) -> R,
    R: BlockReturn,
{
    fn call(&self, mrb: &Mrb, args: &[Value], block: Option<Proc>) -> Result<Value, Error> {
        if self.running.replace(true) {
            return Err(Error::Exception(crate::method::core_exception(
                mrb,
                c"RuntimeError",
                "proc closure called while it is already running",
            )));
        }
        let running = Running(&self.running);
        // SAFETY: the flag admits one call at a time, so this is the only
        // borrow of the closure while it lives.
        let result = (unsafe { &mut *self.body.get() })(mrb, args, block);
        drop(running);
        result.into_block_return(mrb)
    }
}

/// Clears the running flag however the closure leaves, a panic included.
struct Running<'a>(&'a Cell<bool>);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// The C function every Rust-defined proc runs: it reads the body from
/// the proc's environment and runs it under the panic boundary, raising
/// any error to the proc's caller.
unsafe extern "C" fn proc_bridge(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: mruby invokes the bridge with a live state pointer that
    // outlives the call frame.
    let mrb = unsafe { Mrb::borrow_raw(&mrb) };
    // SAFETY: this is the bridge frame the raise contract names.
    let value = unsafe { crate::method::handle_error(mrb, || run_body(mrb)) };
    value.as_raw()
}

/// Run the running proc's body over the call's arguments and block.
fn run_body(mrb: &Mrb) -> Result<Value, Error> {
    // SAFETY: the running proc is one `proc_with_body` built, whose one
    // environment slot holds the carrier of its body.
    let carrier = unsafe { sys::mrb_proc_cfunc_env_get(mrb.as_ptr(), 0) };
    // SAFETY: the carrier's data is the box `proc_with_body` handed
    // over, alive while the running proc keeps the carrier reachable.
    let body = unsafe {
        let rdata = sys::mrb_obj_ptr_func(carrier) as *const sys::RData;
        &*((*rdata).data as *const Box<dyn ProcBody>)
    };
    crate::state::args::with_call(mrb, |args, block| body.call(mrb, args, block))
}

/// What a dump carries beside the compiled instructions.
///
/// The default carries neither — the smallest bytecode that still
/// loads. Ask for `debug_info` where the loaded program's exceptions
/// should answer a source backtrace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DumpOptions {
    /// Carry the line numbers a loaded program's exceptions are
    /// backtraced from.
    pub debug_info: bool,
    /// Carry the local variable names.
    pub locals: bool,
}

impl DumpOptions {
    /// The flag word mruby's dumper reads. It spells the local variable
    /// names as an opt-out, so leaving them behind is what sets a bit.
    fn flags(self) -> u8 {
        let mut flags = if self.locals {
            0
        } else {
            sys::MRB_DUMP_NO_LVAR
        };
        if self.debug_info {
            flags |= sys::MRB_DUMP_DEBUG_INFO;
        }
        flags as u8
    }
}

impl Proc {
    /// The compiled form of this Proc as bytecode, which
    /// `Mrb::load_bytecode` reads back.
    ///
    /// A Proc backed by a C function has no compiled form and comes
    /// back `Err`, as does a dump mruby could not complete.
    pub fn dump(self, mrb: &Mrb, options: DumpOptions) -> Result<Vec<u8>, Error> {
        // SAFETY: `self` is Proc-tagged by the type's invariant, and
        // the shim resolves the flags bitfield and the body union in
        // the C compiler.
        let irep = unsafe { sys::mrb_proc_irep_func(self.as_raw()) };
        if irep.is_null() {
            return Err(Self::undumpable(
                mrb,
                "a Proc backed by a C function carries no bytecode",
            ));
        }

        let mut bin: *mut u8 = core::ptr::null_mut();
        let mut size: usize = 0;
        // SAFETY: `mrb` is live; `irep` belongs to this Proc; mruby
        // writes the buffer it allocated and its length through the two
        // out-parameters.
        let code =
            unsafe { sys::mrb_dump_irep(mrb.as_ptr(), irep, options.flags(), &mut bin, &mut size) };
        if code != sys::MRB_DUMP_OK as core::ffi::c_int || bin.is_null() {
            return Err(Self::undumpable(mrb, "mruby could not dump the Proc"));
        }

        // SAFETY: mruby wrote `size` bytes at `bin`, and the copy is
        // finished before the buffer is released.
        let bytes = unsafe { core::slice::from_raw_parts(bin, size) }.to_vec();
        // SAFETY: the buffer is mruby's allocation and this is its only
        // release; nothing reads it after the copy above.
        unsafe { sys::mrb_free(mrb.as_ptr(), bin.cast()) };
        Ok(bytes)
    }

    /// A dump that produced nothing, reported as the `Err` carrying an
    /// exception that every other failure is reported in.
    fn undumpable(mrb: &Mrb, message: &str) -> Error {
        match mrb.exception_runtime_error() {
            Ok(class) => Error::new(mrb, class, message),
            Err(err) => err,
        }
    }
}

crate::value::value_backed_repr!(Proc);

impl FromValue for Proc {
    #[inline]
    fn from_value(value: Value) -> Option<Self> {
        // SAFETY: the wrap precondition (MRB_TT_PROC tagging) is
        // established by the tag check immediately before it.
        (value.tag() == sys::MRB_TT_PROC).then(|| unsafe { Proc::from_value_unchecked(value) })
    }
}

impl TryConvert for Proc {
    #[inline]
    fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
        Proc::from_value(val).ok_or_else(|| {
            type_error(
                mrb,
                &format!("wrong argument type {} (expected Proc)", val.classname(mrb)),
            )
        })
    }
}
