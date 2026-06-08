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
        #[cfg(mruby_linked)]
        {
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
                    sys::mrb_yield_argv(inner.as_ptr(), block_raw, args.len() as sys::mrb_int, argv)
                };
                Value::from_raw(raw)
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, args);
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{Ccontext, Error, FromValue, IntoValue, Mrb, Proc, Value};

    fn proc_from(mrb: &Mrb, src: &[u8]) -> Proc {
        let cxt = Ccontext::new(mrb, c"proc_test.rb")
            .expect("allocating the compile context must succeed");
        let value = cxt.load_nstring(src);
        assert!(
            mrb.pending_exc().is_nil(),
            "compiling the proc literal must not raise: {}",
            mrb.pending_exc().to_string(mrb)
        );
        Proc::from_value(value).expect("a proc literal carries MRB_TT_PROC")
    }

    #[test]
    fn call_yields_to_the_block_and_returns_its_value() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let block = proc_from(&mrb, b"proc { |x| x + 1 }");

        let got = block
            .call(&mrb, &[41i32.into_value(&mrb)])
            .expect("yielding a non-raising block must come back Ok");

        assert_eq!(i32::from_value(got), Some(42));
    }

    #[test]
    fn call_surfaces_a_raised_exception_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let block = proc_from(&mrb, b"proc { raise 'boom from block' }");

        let err = block
            .call(&mrb, &[])
            .expect_err("a raise inside the block must surface as Err");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("boom from block")),
            Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
        }
        // The VM stays usable after the protected raise.
        let again = proc_from(&mrb, b"proc { 7 }");
        let got = again
            .call(&mrb, &[])
            .expect("the VM must survive the protected raise");
        assert_eq!(i32::from_value(got), Some(7));
    }

    #[test]
    fn from_value_rejects_a_non_proc_value() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A scalar carries no MRB_TT_PROC tag — the downcast rejects
        // instead of wrapping a value `mrb_yield_argv` would misread.
        assert!(Proc::from_value(42i32.into_value(&mrb)).is_none());
    }

    #[test]
    fn as_value_round_trips_through_the_newtype() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let block = proc_from(&mrb, b"proc { 0 }");

        // The reified value is still Proc-tagged and downcasts back.
        let value: Value = block.as_value();
        assert!(Proc::from_value(value).is_some());
    }
}
