//! `DataType<T>`, the data type a carrier is tagged with, the class
//! mark that makes a class allocate data carriers (`CDATA`), and the
//! allocator switch that keeps Ruby from allocating empty ones.
//!
//! A `DataType<T>` is a `'static` descriptor binding an mruby
//! `mrb_data_type` to the Rust type `T` it carries: its release hook
//! drops the boxed `T` when the carrier is garbage-collected. mruby
//! compares descriptors by identity, so a carrier's data type proves its
//! payload's Rust type; `typed_data` wraps and reads through it.

use crate::{Error, Mrb, RClass, ReprValue};
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
/// The descriptor holds mruby's `mrb_data_type` — a release hook plus
/// a type name — beside the `T` marker that types the payload.
pub struct DataType<T> {
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
    /// class name, which `TypedData::class` names.
    pub const fn new(struct_name: &'static core::ffi::CStr) -> Self {
        Self {
            raw: sys::mrb_data_type {
                struct_name: struct_name.as_ptr(),
                dfree: Some(Self::dfree),
            },
            _marker: PhantomData,
        }
    }

    /// mruby's release hook for this type: drop the boxed `T` the
    /// carrier owned. Registered in the `mrb_data_type` and invoked by
    /// the GC (a C frame) when the carrier is collected — so a panic in
    /// `T`'s `Drop` is caught here, since unwinding across that frame is
    /// undefined. The drop has no error channel, so the payload is
    /// discarded.
    unsafe extern "C" fn dfree(_mrb: *mut sys::mrb_state, ptr: *mut core::ffi::c_void) {
        if !ptr.is_null() {
            // SAFETY: `ptr` was produced by `Box::into_raw::<T>` by a
            // wrap or a copy against this same descriptor, so it
            // is a live `Box<T>` the GC is now releasing. The box is the
            // hook's only state, so `AssertUnwindSafe` holds.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(unsafe { Box::from_raw(ptr as *mut T) });
            }));
        }
    }

    /// Pointer to the underlying `mrb_data_type`. mruby stores this in
    /// each carrier and compares it by identity at extraction, so it
    /// must outlive every wrapped object — which the `'static` data type
    /// `TypedData` names guarantees.
    #[inline]
    pub(crate) fn as_raw(&self) -> *const sys::mrb_data_type {
        &self.raw
    }

    /// The name mruby diagnostics show for this data type.
    pub(crate) fn name(&self) -> std::borrow::Cow<'static, str> {
        // SAFETY: `struct_name` came from the `&'static CStr` handed to
        // `DataType::new`.
        unsafe { core::ffi::CStr::from_ptr(self.raw.struct_name) }.to_string_lossy()
    }
}

impl RClass {
    /// Mark this class, and any class later defined from it, so its
    /// instances allocate as data carriers (`MRB_TT_CDATA`) for
    /// `typed_data`'s wraps. Only a class whose instances are plain objects
    /// or already data carriers accepts; a singleton class or a built-in
    /// layout (an exception, a string, a number, …) refuses with a
    /// `TypeError` and stays unmarked, so mruby never reads a carrier as
    /// that layout.
    pub fn set_instance_data_tt(self, mrb: &Mrb) -> Result<(), Error> {
        let tt = crate::class::instance_tt(self.as_internal());
        if tt != sys::MRB_TT_OBJECT && tt != sys::MRB_TT_CDATA {
            return Err(Error::Exception(crate::method::core_exception(
                mrb,
                c"TypeError",
                "can't mark a class to carry Rust data unless its instances are plain objects or data carriers",
            )));
        }
        // SAFETY: `self` originates from the live VM borrowed as `mrb`;
        // the shim only rewrites the class's instance-tt flag bits.
        unsafe { sys::mrb_set_instance_tt_func(self.as_internal(), sys::MRB_TT_CDATA) };
        Ok(())
    }

    /// Undefine the default allocator of this class and of any class
    /// later defined from it, so Ruby's `new` and `allocate` raise while
    /// wraps still allocate. Mirrors magnus's `undef_default_alloc_func`;
    /// a singleton class, which Ruby never allocates through, is left
    /// unchanged.
    pub fn undef_default_alloc_func(self, mrb: &Mrb) {
        // The `&Mrb` borrow is what serializes this flag write: an
        // `RClass` crosses threads on its own, its interpreter does not.
        let _ = mrb;
        if self.as_value().is_class() {
            // SAFETY: `self` is a live plain class of the VM borrowed as
            // `mrb`, the one kind `MRB_UNDEF_ALLOCATOR` accepts; the shim
            // only sets a flag bit.
            unsafe { sys::mrb_undef_allocator_func(self.as_internal()) };
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
