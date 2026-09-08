//! Typed `Proc` newtype around a Proc-tagged `Value`.
//!
//! `Proc` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Proc` is exactly an `mrb_value` known to carry
//! an mruby `Proc` (a block). Construction is by checked `FromValue`
//! downcast or explicit unchecked cast from `Value`.
//!
//! Mirrors magnus's `block::Proc`: the protected `call` that yields to
//! the block lives here.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

/// Typed handle on an mruby `Proc` (a block). `#[repr(transparent)]`
/// over `Value` so the C ABI is preserved.
///
/// Construct via the checked `FromValue` downcast (`Proc::from_value`,
/// tag-discriminated) or `Proc::from_value_unchecked` (assert that a
/// `Value` you already hold is Proc-tagged). Round-trip back to a
/// generic `Value` via `Proc::as_value` for APIs that take any value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Proc(Value);

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

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not
    /// yet migrated. Same conversion ladder as `Value::as_raw`.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// Yield to this block with `args` under exception protection.
    /// The block's normal return is `Ok(value)`; any non-local exit —
    /// a raised exception, or a `break` / `return` object the block
    /// throws — surfaces as `Err` instead of unwinding across FFI,
    /// mirroring `magnus::block::Proc::call`.
    ///
    /// Interpreting a non-local exit (a real `break` versus a `return`
    /// aimed past a frame versus a plain raise) is the caller's
    /// concern: `Value::as_break` discriminates a break and reads its
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
            Value::from_raw(raw)
        })
    }
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
        match mrb.class_get(c"RuntimeError") {
            Ok(class) => Error::new(mrb, class, message),
            Err(err) => err,
        }
    }
}
