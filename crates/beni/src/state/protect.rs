//! Protected execution on `Mrb`: `mrb_protect_error` around a body that
//! only calls into mruby.
//!
//! A raise inside the body long-jumps to the protected frame, so the
//! frames it crosses carry no panic boundary: a `catch_unwind` between
//! the raise and its jump target would make the long-jump undefined
//! behavior. Typed operations run their raising mruby calls here, and so
//! does the public `beni::sys::protect`.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_protect_error(mrb, body, userdata, &error)` — run `body`
    /// inside a protected frame. On success returns `Ok(value)` with the
    /// body's return value; on a raised Ruby exception returns
    /// `Err(Error::Exception)` with the pending exception cleared.
    ///
    /// The trampoline carries no panic boundary, so a raise inside the
    /// body reaches the jump target across plain frames alone. A panic in
    /// the body stops at the `extern "C"` boundary and aborts; a body that
    /// runs Rust which could panic catches it before it gets there.
    pub(crate) fn protect<F>(&self, body: F) -> Result<Value, Error>
    where
        F: FnOnce(&Mrb) -> Value,
    {
        let mut slot: Option<F> = Some(body);

        unsafe extern "C" fn trampoline<F>(
            mrb: *mut sys::mrb_state,
            userdata: *mut core::ffi::c_void,
        ) -> sys::mrb_value
        where
            F: FnOnce(&Mrb) -> Value,
        {
            // SAFETY: userdata is the `&mut Option<F>` from the caller;
            // mrb is the same live state passed to mrb_protect_error.
            let slot: &mut Option<F> = unsafe { &mut *(userdata as *mut Option<F>) };
            let Some(body) = slot.take() else {
                unreachable!("Mrb::protect trampoline invoked twice")
            };
            let mrb_ref = unsafe { Mrb::borrow_raw(&mrb) };
            body(mrb_ref).into_raw()
        }

        let mut error: sys::mrb_bool = false;
        // SAFETY: `self` is alive; `trampoline::<F>` upholds the
        // `mrb_protect_error_func` ABI; `userdata` points to `slot` on
        // this stack frame, which outlives the call. bindgen wraps
        // function-typedef parameters in `Option<…>`, so the trampoline
        // is passed via `Some`.
        let ret = unsafe {
            sys::mrb_protect_error(
                self.as_ptr(),
                Some(trampoline::<F>),
                &mut slot as *mut Option<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        let value = Value::from_raw(ret);
        if error {
            Err(Error::Exception(value))
        } else {
            Ok(value)
        }
    }
}
