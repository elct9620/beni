//! `mrb_protect_error` closure wrapper on `Mrb`.
//!
//! Inherent method that wraps mruby's `mrb_protect_error` so any
//! Ruby exception the body raises is caught and surfaced as
//! `Err(Error::Exception)` instead of long-jumping past the Rust
//! caller, and any Rust panic the body raises is caught at the FFI
//! boundary and surfaced as `Err(Error::Panic)` instead of unwinding
//! into mruby's C frames.

use crate::{Error, Mrb, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

/// Trampoline-slot state for `Mrb::protect`. Starts as `Body`; the
/// trampoline takes the closure out to run it and, when the body
/// panics, parks the panic message back in the slot so the Rust side
/// of the FFI call can surface it as `Error::Panic`.
#[cfg(mruby_linked)]
enum Slot<F> {
    Body(F),
    Taken,
    Panicked(String),
}

#[cfg(mruby_linked)]
use crate::error::panic_message;

impl Mrb {
    /// `mrb_protect_error(mrb, body, userdata, &error)` — run `body`
    /// inside a protected frame. On success returns `Ok(value)` with
    /// the body's return value; on a raised Ruby exception returns
    /// `Err(Error::Exception)`; on a Rust panic inside the body
    /// returns `Err(Error::Panic)` — the panic is caught before it
    /// can unwind into mruby's C frames.
    ///
    /// ## Closure form
    ///
    /// The closure receives a borrowed `&Mrb` (the same VM `self`
    /// points to) so it can call safe methods inside the protected
    /// frame without re-acquiring the borrow. It must return a
    /// `Value` — the protected frame's value is whatever the
    /// closure produces, mirroring mruby's own `body` contract.
    ///
    /// ## Drop semantics on the raise path
    ///
    /// When the closure raises a Ruby exception, mruby long-jumps out
    /// of the body before the closure returns normally. Anything the
    /// closure captured that needs `Drop` to run (heap allocations,
    /// owned strings, etc.) **will not be dropped** on that path —
    /// `setjmp`/`longjmp` does not unwind Rust stack frames. (A Rust
    /// panic is different: `catch_unwind` runs drops normally before
    /// the panic is converted to `Error::Panic`.)
    ///
    /// **Capture `Copy` values only** (`Value` is `Copy`) unless
    /// the rare leak on the raise path is acceptable for the
    /// captured state. The closure-slot pattern below keeps the
    /// per-call overhead allocation-free; only the closure's own
    /// captures are at risk.
    pub fn protect<F>(&self, body: F) -> Result<Value, Error>
    where
        F: FnOnce(&Mrb) -> Value,
    {
        #[cfg(not(mruby_linked))]
        {
            let _ = body;
            crate::not_linked()
        }
        #[cfg(mruby_linked)]
        {
            self.protect_linked(body)
        }
    }

    /// Linked-mode body of `Mrb::protect`, split out because the
    /// trampoline + closure-slot dance reads better without an extra
    /// cfg indentation level.
    #[cfg(mruby_linked)]
    fn protect_linked<F>(&self, body: F) -> Result<Value, Error>
    where
        F: FnOnce(&Mrb) -> Value,
    {
        // Hold the closure in a stack-local slot so the trampoline
        // can take it without owning a heap allocation. The slot's
        // storage outlives the FFI call by virtue of being a local;
        // on the Ruby-raise path the long-jump leaves it as `Taken`
        // (the trampoline already took the closure out) and the
        // subsequent return into Rust drops it cleanly. On the panic
        // path the trampoline parks the message in the slot for the
        // post-call check below.
        let mut slot: Slot<F> = Slot::Body(body);

        unsafe extern "C" fn trampoline<F>(
            mrb: *mut sys::mrb_state,
            userdata: *mut core::ffi::c_void,
        ) -> sys::mrb_value
        where
            F: FnOnce(&Mrb) -> Value,
        {
            // SAFETY: userdata is the `&mut Slot<F>` from the
            // caller; mrb is the same live state passed to
            // mrb_protect_error.
            let slot: &mut Slot<F> = unsafe { &mut *(userdata as *mut Slot<F>) };
            let Slot::Body(body) = core::mem::replace(slot, Slot::Taken) else {
                unreachable!("Mrb::protect trampoline invoked twice")
            };
            let mrb_ref = unsafe { Mrb::borrow_raw(&mrb) };
            // The panic boundary (spec: a panic in an
            // exception-protected closure surfaces as a Rust `Err`):
            // catching here keeps the unwind out of
            // `mrb_protect_error`'s C frame. AssertUnwindSafe matches
            // magnus — the closure is consumed either way, so no
            // observable broken state survives the catch.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(mrb_ref))) {
                Ok(value) => value.into_raw(),
                Err(payload) => {
                    *slot = Slot::Panicked(panic_message(payload));
                    Value::zeroed().into_raw()
                }
            }
        }

        let mut error: sys::mrb_bool = false;
        // SAFETY: `self` is alive; `trampoline::<F>` upholds the
        // `mrb_protect_error_func` ABI; `userdata` points to `slot`
        // on this stack frame which outlives the call. bindgen wraps
        // function-typedef parameters in `Option<…>`, so the
        // trampoline must be passed via `Some`.
        let ret = unsafe {
            sys::mrb_protect_error(
                self.as_ptr(),
                Some(trampoline::<F>),
                &mut slot as *mut Slot<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        if let Slot::Panicked(msg) = slot {
            return Err(Error::Panic(msg));
        }
        let value = Value::from_raw(ret);
        if error {
            Err(Error::Exception(value))
        } else {
            Ok(value)
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;

    #[test]
    fn protect_returns_the_body_value_on_success() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let got = mrb
            .protect(|m| m.str_new(b"ok").as_value())
            .expect("a non-raising body must come back Ok");

        assert_eq!(got.to_string(&mrb), "ok");
    }

    #[test]
    fn protect_surfaces_a_raised_ruby_exception_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .protect(|m| {
                // SAFETY: `m` is the live VM inside the protected frame;
                // `RuntimeError` is a core class so the lookup cannot
                // fail; `mrb_raise` long-jumps to the protect frame and
                // never returns here.
                unsafe {
                    let runtime_error = sys::mrb_class_get(m.as_ptr(), c"RuntimeError".as_ptr());
                    sys::mrb_raise(m.as_ptr(), runtime_error, c"boom from ruby".as_ptr());
                }
                Value::zeroed()
            })
            .expect_err("a raise inside the body must surface as Err");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("boom from ruby")),
            Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
        }
        // The VM stays usable after the protected raise.
        let again = mrb
            .protect(|m| m.str_new(b"alive").as_value())
            .expect("the VM must survive the protected raise");
        assert_eq!(again.to_string(&mrb), "alive");
    }
}
