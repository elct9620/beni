//! Protected execution on `Mrb`: `mrb_protect_error` around a body that
//! only calls into mruby.
//!
//! A raise inside the body long-jumps to the protected frame, so the
//! frames it crosses carry no panic boundary: a `catch_unwind` between
//! the raise and its jump target would make the long-jump undefined
//! behavior. Typed operations run their raising mruby calls here, and so
//! does the public `beni::sys::protect`.

use crate::{sys::AsRawValue, Error, Mrb, ReprValue, Value};
use beni_sys as sys;

impl Mrb {
    /// `mrb_protect_error(mrb, body, userdata, &error)` — run `body`
    /// inside a protected frame. On success returns `Ok` with the body's
    /// return value, typed as the body produced it; on a raised Ruby
    /// exception returns `Err(Error::Exception)` with the pending
    /// exception cleared.
    ///
    /// The trampoline carries no panic boundary, so a raise inside the
    /// body reaches the jump target across plain frames alone. A panic in
    /// the body stops at the `extern "C"` boundary and aborts; a body that
    /// runs Rust which could panic catches it before it gets there.
    pub(crate) fn protect<F, T>(&self, body: F) -> Result<T, Error>
    where
        F: FnOnce(&Mrb) -> T,
        T: ReprValue,
    {
        let mut slot: Option<F> = Some(body);

        // MSVC's long-jump runs the cleanups of the frames it crosses, and a
        // cleanup in an `extern "C"` frame aborts, so this frame holds no
        // value that might need dropping and the body runs one frame up.
        unsafe extern "C" fn trampoline<F, T>(
            mrb: *mut sys::mrb_state,
            userdata: *mut core::ffi::c_void,
        ) -> sys::mrb_value
        where
            F: FnOnce(&Mrb) -> T,
            T: ReprValue,
        {
            // SAFETY: forwarded from `mrb_protect_error`, which passes the
            // state and userdata this call handed it.
            unsafe { run_body::<F, T>(mrb, userdata) }
        }

        unsafe fn run_body<F, T>(
            mrb: *mut sys::mrb_state,
            userdata: *mut core::ffi::c_void,
        ) -> sys::mrb_value
        where
            F: FnOnce(&Mrb) -> T,
            T: ReprValue,
        {
            // SAFETY: userdata is the `&mut Option<F>` from the caller;
            // mrb is the same live state passed to mrb_protect_error.
            let slot: &mut Option<F> = unsafe { &mut *(userdata as *mut Option<F>) };
            let Some(body) = slot.take() else {
                unreachable!("Mrb::protect trampoline invoked twice")
            };
            let mrb_ref = unsafe { Mrb::borrow_raw(&mrb) };
            body(mrb_ref).as_value().as_raw()
        }

        let mut error: sys::mrb_bool = false;
        // SAFETY: `self` is alive; `trampoline::<F, T>` upholds the
        // `mrb_protect_error_func` ABI; `userdata` points to `slot` on
        // this stack frame, which outlives the call. bindgen wraps
        // function-typedef parameters in `Option<…>`, so the trampoline
        // is passed via `Some`.
        let ret = unsafe {
            sys::mrb_protect_error(
                self.as_ptr(),
                Some(trampoline::<F, T>),
                &mut slot as *mut Option<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        let value = Value::from_raw_unchecked(ret);
        if error {
            Err(Error::Exception(value))
        } else {
            // SAFETY: with no raise, `value` is what the trampoline boxed
            // from the body's `T`.
            Ok(unsafe { <T as crate::value::private::ReprValue>::from_value_unchecked(value) })
        }
    }
}
