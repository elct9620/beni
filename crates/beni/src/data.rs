//! Typed `DataType<T>` and the data-carrier (`CDATA`) seam — beni's
//! mirror of magnus's typed-data wrapping.
//!
//! A `DataType<T>` is a `'static` descriptor binding an mruby
//! `mrb_data_type` to the Rust type `T` it carries: its release hook
//! drops the boxed `T` when the carrier is garbage-collected. A class
//! is marked through `RClass::set_instance_data_tt` so its instances
//! allocate as data carriers; `RClass::data_wrap` boxes a `T` into a
//! fresh instance of that class — fallibly, since allocating against an
//! unmarked class raises a `TypeError` that surfaces as `Err` with the
//! box reclaimed; `Value::data_get` extracts `&T` back, type-checked
//! against the descriptor the value was wrapped under.
//!
//! The wrapped value's lifetime belongs to the mruby GC — the release
//! hook runs when the carrier is collected (or the VM closes).
//! Extraction routes through mruby's own `mrb_data_check_get_ptr`,
//! which compares the descriptor by identity: a value of a different
//! data type, or a non-data value, yields `None` rather than a misread
//! pointer.

use crate::{Mrb, RClass, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;
use core::marker::PhantomData;

/// A `'static` descriptor binding an mruby data type to the Rust type
/// `T` it carries. Declare one per wrapped type as a `static`:
///
/// ```ignore
/// use beni::DataType;
/// static REGEX_TYPE: DataType<MyRegex> = DataType::new(c"Regexp");
/// ```
///
/// In linked builds the descriptor holds mruby's `mrb_data_type` (a
/// release hook plus a type name); in placeholder builds it holds only
/// the `T` marker, since no mruby symbols exist to describe.
pub struct DataType<T> {
    #[cfg(mruby_linked)]
    raw: sys::mrb_data_type,
    _marker: PhantomData<T>,
}

// SAFETY: the descriptor carries no `T` value — only a release-hook
// function pointer and a `'static` type name, both immutable plain
// data. Reaching it from a thread reaches no `T`, so both markers hold
// regardless of `T`: one descriptor serves every interpreter, on
// whichever thread each is reached from.
unsafe impl<T> Send for DataType<T> {}
unsafe impl<T> Sync for DataType<T> {}

impl<T> DataType<T> {
    /// Construct a descriptor naming the wrapped type. `struct_name`
    /// labels the data type in mruby diagnostics; it is not the Ruby
    /// class name (the class is chosen at `RClass::data_wrap` time).
    pub const fn new(struct_name: &'static core::ffi::CStr) -> Self {
        #[cfg(mruby_linked)]
        {
            Self {
                raw: sys::mrb_data_type {
                    struct_name: struct_name.as_ptr(),
                    dfree: Some(Self::dfree),
                },
                _marker: PhantomData,
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = struct_name;
            Self {
                _marker: PhantomData,
            }
        }
    }

    /// mruby's release hook for this type: drop the boxed `T` the
    /// carrier owned. Registered in the `mrb_data_type` and invoked by
    /// the GC (a C frame) when the carrier is collected — so a panic in
    /// `T`'s `Drop` is caught here, since unwinding across that frame is
    /// undefined. The drop has no error channel, so the payload is
    /// discarded.
    #[cfg(mruby_linked)]
    unsafe extern "C" fn dfree(_mrb: *mut sys::mrb_state, ptr: *mut core::ffi::c_void) {
        if !ptr.is_null() {
            // SAFETY: `ptr` was produced by `Box::into_raw::<T>` in
            // `RClass::data_wrap` against this same descriptor, so it
            // is a live `Box<T>` the GC is now releasing. The box is the
            // hook's only state, so `AssertUnwindSafe` holds.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(unsafe { Box::from_raw(ptr as *mut T) });
            }));
        }
    }

    /// Pointer to the underlying `mrb_data_type`. mruby stores this in
    /// each carrier and compares it by identity at extraction, so it
    /// must outlive every wrapped object — which the `'static` bound on
    /// the callers (`data_wrap` / `data_get`) guarantees.
    #[cfg(mruby_linked)]
    #[inline]
    fn as_raw(&self) -> *const sys::mrb_data_type {
        &self.raw
    }
}

impl RClass {
    /// Mark this class so its instances allocate as data carriers
    /// (`MRB_TT_CDATA`). Call once at class setup, before wrapping any
    /// instance through `RClass::data_wrap`.
    #[inline]
    pub fn set_instance_data_tt(self, _mrb: &Mrb) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` originates from the live VM borrowed as
            // `_mrb`; the shim only rewrites the class's instance-tt
            // flag bits.
            unsafe { sys::mrb_set_instance_tt_func(self.as_raw(), sys::MRB_TT_CDATA) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = _mrb;
            crate::not_linked()
        }
    }

    /// Box `value` and wrap it as a fresh instance of this class,
    /// carrying it under `ty`. Fallible: a class marked through
    /// `RClass::set_instance_data_tt` yields `Ok`, and the mruby GC owns
    /// the box from there — its release hook drops the `T` when the
    /// instance is collected. A class that cannot carry a data carrier —
    /// one never marked — makes the allocation raise a `TypeError`, which
    /// surfaces as `Err` rather than unwinding across the boundary; the
    /// box not yet handed to any carrier is reclaimed, never leaked.
    /// Mirrors `magnus`'s typed-data wrapping.
    ///
    /// The payload travels with the interpreter and is dropped by the
    /// release hook on whichever thread reaches it, so only a `Send`
    /// payload wraps:
    ///
    /// ```
    /// # use beni::{DataType, Mrb, RClass};
    /// struct Counter(u32);
    /// static COUNTER: DataType<Counter> = DataType::new(c"Counter");
    /// fn wrap(mrb: &Mrb, class: RClass) {
    ///     let _ = class.data_wrap(mrb, Counter(0), &COUNTER);
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// # use beni::{DataType, Mrb, RClass};
    /// struct Bare(*const ());
    /// static BARE: DataType<Bare> = DataType::new(c"Bare");
    /// fn wrap(mrb: &Mrb, class: RClass) {
    ///     let _ = class.data_wrap(mrb, Bare(core::ptr::null()), &BARE);
    /// }
    /// ```
    #[inline]
    pub fn data_wrap<T: Send>(
        self,
        mrb: &Mrb,
        value: T,
        ty: &'static DataType<T>,
    ) -> Result<Value, crate::Error> {
        #[cfg(mruby_linked)]
        {
            let ptr = Box::into_raw(Box::new(value)) as *mut core::ffi::c_void;
            // `ptr` is `Copy`, so the closure captures a copy while this
            // frame keeps the original for the reclaim path. On success
            // mruby's allocation owns the box; on the raise path the
            // allocation never attached it to any object, so this frame
            // reclaims the still-orphaned box exactly once.
            let wrapped = mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // is from the same VM; `ptr` is a freshly leaked `Box<T>`;
                // `ty` is `'static`, so its descriptor outlives the
                // carrier. `mrb_data_object_alloc` allocates the carrier,
                // which raises a `TypeError` when `self` was never marked
                // to carry a data carrier — caught by `protect`.
                let rdata = unsafe {
                    sys::mrb_data_object_alloc(mrb.as_ptr(), self.as_raw(), ptr, ty.as_raw())
                };
                // SAFETY: `rdata` is a live object pointer just allocated
                // against this VM; `mrb_obj_value` reifies it.
                Value::from_raw(unsafe { sys::mrb_obj_value(rdata as *mut core::ffi::c_void) })
            });
            if wrapped.is_err() {
                // SAFETY: the allocation raised before handing the box to
                // any carrier, so no GC owner exists and `ptr` is still the
                // sole owner of the live `Box<T>`. Reclaiming it here drops
                // the `T` once; the success path never reaches this, so the
                // box is freed exactly once across both paths.
                drop(unsafe { Box::from_raw(ptr as *mut T) });
            }
            wrapped
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, value, ty);
            crate::not_linked()
        }
    }
}

impl Value {
    /// Extract `&T` from a data carrier wrapped under `ty`. Returns
    /// `None` when `self` is not a data carrier or carries a different
    /// data type — the identity check is mruby's own
    /// `mrb_data_check_get_ptr`, so a mismatched type never misreads
    /// the pointer.
    ///
    /// The borrow is bounded by the `&Mrb` borrow: the carried value
    /// lives as long as its carrier stays reachable, which the consumer
    /// upholds under the GC validity rule (as for any borrowed mruby
    /// payload).
    ///
    /// The borrow is read-only — mutating the payload goes through
    /// interior mutability (`RefCell`/`Cell`) inside `T`, as in magnus's
    /// typed data.
    #[inline]
    pub fn data_get<'a, T>(self, mrb: &'a Mrb, ty: &'static DataType<T>) -> Option<&'a T> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` originates from the same
            // VM. `mrb_data_check_get_ptr` returns NULL unless `self`
            // carries exactly `ty`'s data type.
            let ptr =
                unsafe { sys::mrb_data_check_get_ptr(mrb.as_ptr(), self.into_raw(), ty.as_raw()) }
                    as *const T;
            if ptr.is_null() {
                None
            } else {
                // SAFETY: the identity check above confirms the pointer
                // was produced by `data_wrap::<T>`, so it is a live `T`
                // owned by the carrier; the borrow is bounded by `'a`.
                Some(unsafe { &*ptr })
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, ty);
            crate::not_linked()
        }
    }

    /// Box `value` and install it as this carrier's payload under `ty`,
    /// handing the box to the mruby GC — the same ownership transfer as
    /// `RClass::data_wrap`, but into an existing instance instead of a
    /// fresh one. This is the seam an `initialize_copy` uses to copy a
    /// typed object: a `dup` or `clone` hands it the bare carrier it just
    /// allocated, and the body installs a clone of the original's state.
    ///
    /// The install targets a CDATA carrier — a class marked through
    /// `RClass::set_instance_data_tt`. It does not release a payload
    /// already present, so re-running it over a live carrier leaks the
    /// previous box; applied to a non-carrier value it does nothing.
    #[inline]
    pub fn data_reinit<T: Send>(self, _mrb: &Mrb, value: T, ty: &'static DataType<T>) {
        #[cfg(mruby_linked)]
        {
            // `mrb_data_init` writes through the carrier's `RData`, so it is
            // defined only on a CDATA value; `is_data` mirrors mruby's
            // `mrb_data_p` exactly, so gating on it keeps that write sound and
            // leaves a non-carrier value untouched.
            if self.is_data() {
                let ptr = Box::into_raw(Box::new(value)) as *mut core::ffi::c_void;
                // SAFETY: `self` is a CDATA carrier from the live VM borrowed
                // as `_mrb`; `ptr` is a freshly leaked `Box<T>` handed to
                // mruby, which releases it via `ty`'s release hook; `ty` is
                // `'static`, so its descriptor outlives the carrier.
                unsafe { sys::mrb_data_init(self.into_raw(), ptr, ty.as_raw()) };
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (value, ty);
            crate::not_linked()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_descriptor_crosses_threads_whatever_it_describes() {
        // The descriptor carries no `T`, so it crosses even where the
        // payload it names could not. A field of its own would end
        // that, and end it here.
        fn crosses<T: Send + Sync>() {}
        crosses::<crate::DataType<*const ()>>();
    }
}
