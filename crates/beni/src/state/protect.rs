//! Protected execution on `Mrb`: `mrb_protect_error` around a body that
//! only calls into mruby.
//!
//! A raise inside the body long-jumps to the protected frame, so the
//! frames it crosses carry no panic boundary: a `catch_unwind` between
//! the raise and its jump target would make the long-jump undefined
//! behavior. Typed operations run their raising mruby calls here, and so
//! does the public `beni::sys::protect`.

use crate::{
    Array, Error, ExceptionClass, Hash, Mrb, RClass, RModule, RString, Range, Symbol, Value,
};
use beni_sys as sys;

/// A typed handle that rides through the protect frame as the `Value` it
/// boxes and is unwrapped unchecked on the way out — magnus's `ReprValue`,
/// the carrier its crate-private `protect` is generic over. mruby's
/// protected body answers only an `mrb_value`, so a body producing a class
/// pointer or a symbol id hands back the handle that boxes it.
pub(crate) trait ReprValue: Copy {
    /// Box `self` as the `Value` it stands for.
    fn as_value(self) -> Value;

    /// Unwrap `v` as `Self` without checking its tag.
    ///
    /// # Safety
    ///
    /// `v` must be a value `Self::as_value` produced.
    unsafe fn from_value_unchecked(v: Value) -> Self;
}

impl ReprValue for Value {
    fn as_value(self) -> Value {
        self
    }

    unsafe fn from_value_unchecked(v: Value) -> Self {
        v
    }
}

/// The handles that are a tagged `Value` underneath already carry both
/// directions as inherent methods.
macro_rules! value_backed_repr {
    ($($handle:ty),*) => {$(
        impl ReprValue for $handle {
            fn as_value(self) -> Value {
                <$handle>::as_value(self)
            }

            unsafe fn from_value_unchecked(v: Value) -> Self {
                // SAFETY: forwarded from the caller.
                unsafe { <$handle>::from_value_unchecked(v) }
            }
        }
    )*};
}

value_backed_repr!(Symbol, RString, Array, Hash, Range);

/// The class handles hold the `RClass *` itself, boxed with
/// `mrb_obj_value` and recovered with the class-pointer unbox.
macro_rules! class_backed_repr {
    ($($handle:ty => $from_raw:path),*) => {$(
        impl ReprValue for $handle {
            fn as_value(self) -> Value {
                // SAFETY: `mrb_obj_value` only boxes the pointer.
                Value::from_raw(unsafe {
                    sys::mrb_obj_value(self.as_raw() as *mut core::ffi::c_void)
                })
            }

            unsafe fn from_value_unchecked(v: Value) -> Self {
                // SAFETY: `v` boxes a class pointer, by the caller's
                // contract.
                $from_raw(unsafe { v.as_class_ptr() })
            }
        }
    )*};
}

class_backed_repr!(
    RClass => RClass::from_raw,
    RModule => RModule::from_raw,
    ExceptionClass => ExceptionClass::from_raw_unchecked
);

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
            body(mrb_ref).as_value().into_raw()
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
                Some(trampoline::<F, T>),
                &mut slot as *mut Option<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        let value = Value::from_raw(ret);
        if error {
            Err(Error::Exception(value))
        } else {
            // SAFETY: with no raise, `value` is what the trampoline boxed
            // from the body's `T`.
            Ok(unsafe { T::from_value_unchecked(value) })
        }
    }
}
