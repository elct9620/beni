//! What every test here needs before it can ask anything.

use beni::{FromValue, IntoValue, Mrb, RString, ReprValue, Value};

/// A fresh interpreter. Every test opens one, and a failure here is
/// the archive missing rather than the case failing, so the message
/// names that rather than the file name one toolchain gives it.
pub fn open_mrb() -> Mrb {
    Mrb::open().expect("Mrb::open failed — is an mruby archive linked?")
}

/// Whether two handles name the same object — identity through the
/// values they stand for, the comparison a consumer has for handles
/// that carry no raw pointer of their own.
pub fn same_object(mrb: &Mrb, a: impl IntoValue, b: impl IntoValue) -> bool {
    a.into_value(mrb).is_equal(mrb, b.into_value(mrb))
}

/// The Hashes the heap holds, live or not yet collected.
pub fn hashes_on_the_heap(mrb: &Mrb) -> usize {
    unsafe extern "C" fn count(
        _mrb: *mut beni::sys::mrb_state,
        obj: *mut beni::sys::RBasic,
        data: *mut core::ffi::c_void,
    ) -> core::ffi::c_int {
        // SAFETY: the walk hands each heap object, boxed as the value it is.
        let value = unsafe {
            <Value as beni::sys::FromRawValue>::from_raw(beni::sys::mrb_obj_value(obj.cast()))
        };
        if beni::RHash::from_value(value).is_some() {
            // SAFETY: `data` is the counter the caller passed.
            unsafe { *data.cast::<usize>() += 1 };
        }
        beni::sys::MRB_EACH_OBJ_OK as core::ffi::c_int
    }
    let mut hashes = 0usize;
    // SAFETY: `mrb` is alive; the callback reads each object and writes
    // only the counter, which outlives the walk.
    unsafe {
        beni::sys::mrb_objspace_each_objects(
            mrb.as_ptr(),
            Some(count),
            (&mut hashes as *mut usize).cast(),
        )
    };
    hashes
}

/// A string's bytes as an owned `Vec<u8>`, the way a consumer without
/// the `bytes` feature reads them — through the `FromValue` conversion.
pub trait OwnedBytes {
    fn owned_bytes(self) -> Vec<u8>;
}

impl OwnedBytes for RString {
    fn owned_bytes(self) -> Vec<u8> {
        Vec::<u8>::from_value(self.as_value()).expect("a string handle is String-tagged")
    }
}

/// Whether a value converts into the handle `T` — a downcast's answer to
/// "what type is this?", read postfix in an assertion.
pub trait Is {
    fn is<T: FromValue>(self) -> bool;
}

impl Is for Value {
    fn is<T: FromValue>(self) -> bool {
        T::from_value(self).is_some()
    }
}
