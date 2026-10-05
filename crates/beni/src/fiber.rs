//! Creating and resuming fibers — the operations magnus's `Fiber` and
//! `Ruby::fiber_new` carry, gated behind the `fiber` capability
//! feature because mruby keeps fibers in its `mruby-fiber` gem.
//!
//! mruby switches fibers inside its VM rather than between native
//! stacks, so a fiber's body is a Ruby-defined block and a Rust body
//! never runs on a fiber of its own (`mruby-fiber/src/fiber.c`
//! raises `FiberError` for a block backed by a C function).

use crate::value::private::ReprValue as _;
use crate::{sys::AsRawValue, Error, Fiber, Mrb, Proc, ReprValue, TryConvert, Value};
use beni_sys as sys;

impl Mrb {
    /// A fiber that runs `block` on its first resume, mirroring magnus's
    /// `Ruby::fiber_new`. magnus takes a Rust function for the body; an
    /// mruby fiber runs Ruby alone, so the body is a `Proc`, and one
    /// backed by a C function is refused with `FiberError`.
    pub fn fiber_new(&self, block: Proc) -> Result<Fiber, Error> {
        let block_raw = block.as_value().as_raw();
        self.protect(|inner| {
            // SAFETY: `inner` is the live VM inside the protected frame;
            // `block_raw` is Proc-tagged, so its object header is an
            // `RProc`.
            let raw = unsafe {
                let proc_ptr = sys::mrb_obj_ptr_func(block_raw) as *const sys::RProc;
                sys::mrb_fiber_new(inner.as_ptr(), proc_ptr)
            };
            // SAFETY: `mrb_fiber_new` answers a Fiber object or raises.
            unsafe { Fiber::from_value_unchecked(Value::from_raw_unchecked(raw)) }
        })
    }
}

impl Fiber {
    /// Start this fiber with `args` as its block's arguments, or
    /// continue it with `args` as the value its pending yield answers,
    /// mirroring magnus's `Fiber::resume`. Answers the value the fiber
    /// next yields, or its block's value once the block finishes. A
    /// fiber that has finished, is running or already resumed, was
    /// transferred to, or was never initialized surfaces `FiberError`;
    /// an exception the block raises surfaces as the `Err` carrying it.
    pub fn resume<T: TryConvert>(self, mrb: &Mrb, args: &[Value]) -> Result<T, Error> {
        let fiber_raw = self.as_value().as_raw();
        let value = mrb.protect(|inner| {
            // `Value` is `#[repr(transparent)]` over `mrb_value`, so the
            // slice is mruby's argv as-is.
            let argv = args.as_ptr() as *const sys::mrb_value;
            // SAFETY: `inner` is the live VM inside the protected frame;
            // `fiber_raw` is Fiber-tagged; every `args` entry comes from
            // the same VM and the slice outlives the call.
            let raw = unsafe {
                sys::mrb_fiber_resume(
                    inner.as_ptr(),
                    fiber_raw,
                    sys::mrb_int::try_from(args.len()).unwrap_or(sys::mrb_int::MAX),
                    argv,
                )
            };
            Value::from_raw_unchecked(raw)
        })?;
        T::try_convert(value, mrb)
    }

    /// Whether this fiber can still be resumed, mirroring magnus's
    /// `Fiber::is_alive`. A fiber `Fiber.allocate` left uninitialized
    /// answers `FiberError` rather than a bool, as mruby's
    /// `mrb_fiber_alive_p` raises for it.
    pub fn is_alive(self, mrb: &Mrb) -> Result<bool, Error> {
        let fiber_raw = self.as_value().as_raw();
        mrb.protect(|inner| {
            // SAFETY: `inner` is the live VM inside the protected frame;
            // `fiber_raw` is Fiber-tagged.
            Value::from_raw_unchecked(unsafe { sys::mrb_fiber_alive_p(inner.as_ptr(), fiber_raw) })
        })
        .map(Value::to_bool)
    }
}
