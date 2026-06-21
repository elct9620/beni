//! `mrb_protect_error` closure wrapper on `Mrb`.
//!
//! Inherent method that wraps mruby's `mrb_protect_error` so any
//! Ruby exception the body raises is caught and surfaced as
//! `Err(Error::Exception)` instead of long-jumping past the Rust
//! caller, and any Rust panic the body raises is caught at the FFI
//! boundary and surfaced as `Err(Error::Panic)` instead of unwinding
//! into mruby's C frames.

use crate::{Error, Mrb, RClass, Value};
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

    /// Run `body` under exception protection and recover a caught
    /// exception with `handler`, mirroring a Ruby `begin`/`rescue` over
    /// the exception classes in `classes`. A Rust-native composition of
    /// `Mrb::protect` and `Value::is_kind_of` — no new bound C symbol.
    ///
    /// `body`'s normal completion is the `Ok` value. When `body` raises
    /// an exception that is an instance of any class in `classes`,
    /// `handler` runs and its result — `Ok` or `Err` — is the outcome;
    /// `handler` receives the caught exception as a `Value` and runs on
    /// a clean VM (`protect` already cleared the pending exception as it
    /// surfaced the `Err`). Because `handler` also runs under `protect`,
    /// a raise inside it surfaces as the handler's `Err` rather than
    /// long-jumping past the caller.
    ///
    /// What `rescue` does **not** catch:
    ///
    /// - An exception that is an instance of no class in `classes`
    ///   propagates unchanged as `body`'s `Err`. An empty `classes`
    ///   therefore rescues nothing; a caller wanting the bare-`rescue`
    ///   default names `StandardError` in the list.
    /// - A Rust panic inside `body` is not a Ruby exception: it
    ///   surfaces as the panic `Err` regardless of `classes`, honoring
    ///   the same no-long-jump contract `Mrb::protect` carries.
    ///
    /// The same "capture `Copy` values only" caveat as `Mrb::protect`
    /// applies to `body`: a raised exception long-jumps out before the
    /// closure returns, so non-`Copy` captures are not dropped on that
    /// path.
    pub fn rescue<F, G>(&self, classes: &[RClass], body: F, handler: G) -> Result<Value, Error>
    where
        F: FnOnce(&Mrb) -> Value,
        G: FnOnce(&Mrb, Value) -> Value,
    {
        match self.protect(body) {
            Ok(value) => Ok(value),
            Err(Error::Exception(exc))
                if classes.iter().any(|class| exc.is_kind_of(self, *class)) =>
            {
                self.protect(|mrb| handler(mrb, exc))
            }
            Err(other) => Err(other),
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

    #[test]
    fn protect_surfaces_a_panicking_body_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .protect(|_| panic!("boom from rust"))
            .expect_err("a panic inside the body must surface as Err");

        match err {
            Error::Panic(msg) => assert!(msg.contains("boom from rust")),
            Error::Exception(_) => panic!("a Rust panic must surface as Error::Panic"),
        }
        // The VM stays usable after the caught panic.
        let again = mrb
            .protect(|m| m.str_new(b"alive").as_value())
            .expect("the VM must survive the caught panic");
        assert_eq!(again.to_string(&mrb), "alive");
    }

    /// Raise an instance of the named core exception class inside a
    /// protected body, long-jumping to the protect frame the way a real
    /// Ruby raise does (mirrors the `protect` raise test).
    #[cfg(mruby_linked)]
    fn raise_named(m: &Mrb, class: &core::ffi::CStr, message: &core::ffi::CStr) -> Value {
        // SAFETY: `m` is the live VM; the named classes are core so the
        // lookup cannot fail; `mrb_raise` long-jumps and never returns.
        unsafe {
            let class = sys::mrb_class_get(m.as_ptr(), class.as_ptr());
            sys::mrb_raise(m.as_ptr(), class, message.as_ptr());
        }
        Value::zeroed()
    }

    #[test]
    fn rescue_returns_the_body_value_when_it_does_not_raise() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let standard_error = mrb
            .class_get(c"StandardError")
            .expect("StandardError is a core class");

        let got = mrb
            .rescue(
                &[standard_error],
                |m| m.str_new(b"body ok").as_value(),
                |_, _| panic!("the handler must not run when the body succeeds"),
            )
            .expect("a non-raising body comes back Ok");

        assert_eq!(got.to_string(&mrb), "body ok");
    }

    #[test]
    fn rescue_runs_the_handler_on_a_clean_vm_for_a_matching_exception() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let runtime_error = mrb
            .class_get(c"RuntimeError")
            .expect("RuntimeError is a core class");

        // RuntimeError is a StandardError, so a StandardError filter matches it.
        let standard_error = mrb
            .class_get(c"StandardError")
            .expect("StandardError is a core class");

        let got = mrb
            .rescue(
                &[standard_error],
                |m| raise_named(m, c"RuntimeError", c"boom"),
                |m, exc| {
                    // The handler receives the caught exception itself.
                    assert!(exc.is_kind_of(m, runtime_error));
                    assert!(exc.to_string(m).contains("boom"));
                    // The handler runs on a clean VM: no pending exception
                    // remains on the handle, and a fresh allocation works.
                    assert!(m.pending_exc().is_nil());
                    m.str_new(b"handled").as_value()
                },
            )
            .expect("a matching exception is rescued by the handler");

        assert_eq!(got.to_string(&mrb), "handled");
        // The VM stays usable afterwards.
        let again = mrb
            .protect(|m| m.str_new(b"alive").as_value())
            .expect("the VM must survive a rescued exception");
        assert_eq!(again.to_string(&mrb), "alive");
    }

    #[test]
    fn rescue_propagates_an_exception_outside_the_class_list() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        // TypeError is not a kind of ArgumentError, so the filter misses.
        let argument_error = mrb
            .class_get(c"ArgumentError")
            .expect("ArgumentError is a core class");

        let err = mrb
            .rescue(
                &[argument_error],
                |m| raise_named(m, c"TypeError", c"wrong type"),
                |_, _| panic!("the handler must not run for an unmatched exception"),
            )
            .expect_err("an unmatched exception propagates as the body's Err");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("wrong type")),
            Error::Panic(_) => panic!("an unmatched Ruby raise stays Error::Exception"),
        }
    }

    #[test]
    fn rescue_surfaces_a_handler_raise_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let standard_error = mrb
            .class_get(c"StandardError")
            .expect("StandardError is a core class");

        let err = mrb
            .rescue(
                &[standard_error],
                |m| raise_named(m, c"RuntimeError", c"boom"),
                |m, _| raise_named(m, c"RuntimeError", c"handler boom"),
            )
            .expect_err("a handler that raises surfaces as Err");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("handler boom")),
            Error::Panic(_) => panic!("a handler Ruby raise stays Error::Exception"),
        }
    }

    #[test]
    fn rescue_surfaces_a_handler_panic_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let standard_error = mrb
            .class_get(c"StandardError")
            .expect("StandardError is a core class");

        // A matching exception routes into the handler, which then panics.
        // The handler runs under exception protection, so the panic is caught
        // at the FFI boundary as Error::Panic rather than being rescued.
        let err = mrb
            .rescue(
                &[standard_error],
                |m| raise_named(m, c"RuntimeError", c"boom"),
                |_, _| panic!("boom from handler"),
            )
            .expect_err("a handler panic surfaces as Err, not rescued");

        match err {
            Error::Panic(msg) => assert!(msg.contains("boom from handler")),
            Error::Exception(_) => panic!("a handler Rust panic must surface as Error::Panic"),
        }
        // The VM stays usable after the caught handler panic.
        let again = mrb
            .protect(|m| m.str_new(b"alive").as_value())
            .expect("the VM must survive the caught handler panic");
        assert_eq!(again.to_string(&mrb), "alive");
    }

    #[test]
    fn rescue_with_an_empty_class_list_rescues_nothing() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .rescue(
                &[],
                |m| raise_named(m, c"RuntimeError", c"boom"),
                |_, _| panic!("an empty class list never rescues"),
            )
            .expect_err("an empty class list lets every exception propagate");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("boom")),
            Error::Panic(_) => panic!("the raise stays Error::Exception"),
        }
    }

    #[test]
    fn rescue_does_not_catch_a_body_panic() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let standard_error = mrb
            .class_get(c"StandardError")
            .expect("StandardError is a core class");

        let err = mrb
            .rescue(
                &[standard_error],
                |_| panic!("boom from rust"),
                |_, _| panic!("a panic is not a Ruby exception and is never rescued"),
            )
            .expect_err("a body panic surfaces as Err, not rescued");

        match err {
            Error::Panic(msg) => assert!(msg.contains("boom from rust")),
            Error::Exception(_) => panic!("a Rust panic must surface as Error::Panic"),
        }
        // The VM stays usable after the caught panic.
        let again = mrb
            .protect(|m| m.str_new(b"alive").as_value())
            .expect("the VM must survive the caught panic");
        assert_eq!(again.to_string(&mrb), "alive");
    }
}
