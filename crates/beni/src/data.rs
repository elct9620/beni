//! Typed `DataType<T>` and the data-carrier (`CDATA`) seam — beni's
//! mirror of magnus's typed-data wrapping.
//!
//! A `DataType<T>` is a `'static` descriptor binding an mruby
//! `mrb_data_type` to the Rust type `T` it carries: its release hook
//! drops the boxed `T` when the carrier is garbage-collected. A class
//! is marked through `RClass::set_instance_data_tt` so its instances
//! allocate as data carriers; `RClass::data_wrap` boxes a `T` into a
//! fresh instance of that class; `Value::data_get` extracts `&T` back,
//! type-checked against the descriptor the value was wrapped under.
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

// SAFETY: the descriptor is shared as a `'static` but carries no `T`
// value — only a release-hook function pointer and a `'static` type
// name, both immutable plain data. Sharing it across threads shares no
// `T`, so `Sync` holds regardless of `T`; the bound only lets the
// descriptor live in a `static` (mruby itself is single-threaded).
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
    /// the GC when the carrier is collected.
    #[cfg(mruby_linked)]
    unsafe extern "C" fn dfree(_mrb: *mut sys::mrb_state, ptr: *mut core::ffi::c_void) {
        if !ptr.is_null() {
            // SAFETY: `ptr` was produced by `Box::into_raw::<T>` in
            // `RClass::data_wrap` against this same descriptor, so it
            // is a live `Box<T>` the GC is now releasing.
            drop(unsafe { Box::from_raw(ptr as *mut T) });
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
    /// carrying it under `ty`. The mruby GC owns the box from here: its
    /// release hook drops the `T` when the instance is collected. The
    /// class should have been marked through
    /// `RClass::set_instance_data_tt`.
    #[inline]
    pub fn data_wrap<T>(self, mrb: &Mrb, value: T, ty: &'static DataType<T>) -> Value {
        #[cfg(mruby_linked)]
        {
            let ptr = Box::into_raw(Box::new(value)) as *mut core::ffi::c_void;
            // SAFETY: `mrb` is alive; `self` is from the same VM; `ptr`
            // is a freshly leaked `Box<T>` handed to mruby, which will
            // release it via `ty`'s release hook; `ty` is `'static`, so
            // its descriptor outlives the carrier.
            let rdata = unsafe {
                sys::mrb_data_object_alloc(mrb.as_ptr(), self.as_raw(), ptr, ty.as_raw())
            };
            // SAFETY: `rdata` is a live object pointer just allocated
            // against this VM; `mrb_obj_value` reifies it.
            Value::from_raw(unsafe { sys::mrb_obj_value(rdata as *mut core::ffi::c_void) })
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
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;
    use crate::Mrb;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Payload with no observable drop — exercises wrap / get / type
    /// checking without touching the drop-probe counter.
    struct Holder {
        tag: i32,
    }

    static HOLDER_TYPE: DataType<Holder> = DataType::new(c"BeniHolder");
    static OTHER_TYPE: DataType<Holder> = DataType::new(c"BeniOtherHolder");

    #[test]
    fn data_wrap_roundtrips_and_get_is_type_checked() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb
            .define_class(c"BeniDataHolder", mrb.object_class())
            .expect("defining the carrier class must succeed");
        class.set_instance_data_tt(&mrb);

        let obj = class.data_wrap(&mrb, Holder { tag: 7 }, &HOLDER_TYPE);
        assert!(obj.is_data(), "a wrapped carrier reports the data tag");

        let got = obj
            .data_get(&mrb, &HOLDER_TYPE)
            .expect("the matching data type extracts");
        assert_eq!(got.tag, 7);

        // A different descriptor (distinct `mrb_data_type` identity) and
        // a non-data value both reject instead of misreading the pointer.
        assert!(
            obj.data_get(&mrb, &OTHER_TYPE).is_none(),
            "a different data type must not extract"
        );
        assert!(
            mrb.str_new(b"x").data_get(&mrb, &HOLDER_TYPE).is_none(),
            "a non-data value must not extract"
        );
    }

    /// Drop probe with its own counter — kept distinct from `Holder` so
    /// the roundtrip test's VM teardown cannot perturb the assertion.
    static PROBE_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct DropProbe;
    impl Drop for DropProbe {
        fn drop(&mut self) {
            PROBE_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    static PROBE_TYPE: DataType<DropProbe> = DataType::new(c"BeniDropProbe");

    #[test]
    fn release_hook_drops_the_boxed_value_on_close() {
        PROBE_DROPS.store(0, Ordering::SeqCst);
        {
            let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
            let class = mrb
                .define_class(c"BeniDropHolder", mrb.object_class())
                .expect("defining the carrier class must succeed");
            class.set_instance_data_tt(&mrb);

            // Root the carrier so it survives until close, then let the
            // VM drop: `mrb_close` sweeps it and invokes the release hook.
            let obj = class.data_wrap(&mrb, DropProbe, &PROBE_TYPE);
            let slot = mrb.intern_cstr(c"$beni_data_probe");
            mrb.gv_set(slot, obj);
        }
        assert_eq!(
            PROBE_DROPS.load(Ordering::SeqCst),
            1,
            "the release hook must drop the boxed payload exactly once"
        );
    }
}
