//! Typed `RString` newtype around a String-tagged `Value`.
//!
//! `RString` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `RString` is exactly an `mrb_value` known to
//! carry an mruby `String`. The String tag the newtype guarantees is
//! what lets `cat` and `to_bytes` be safe and frees `as_bytes` of the
//! tag obligation `Value` could not discharge.
//!
//! Mirrors magnus's `src/r_string.rs`: string factories live on `Mrb`
//! (`str_new`, `str_new_cstr`), per-string ops (`cat`, `as_bytes`,
//! `to_bytes`) live here.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

/// Typed handle on an mruby `String`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::str_new` / `Mrb::str_new_cstr` (fresh string),
/// the checked `FromValue` downcast (`RString::from_value`,
/// tag-discriminated), or `RString::from_value_unchecked` (assert that
/// a `Value` you already hold is String-tagged). Round-trip back to a
/// generic `Value` via `RString::as_value` for APIs that take any
/// value.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct RString(Value);

impl RString {
    /// Wrap a `Value` that the caller has already determined to be
    /// String-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_str_new`).
    ///
    /// # Safety
    ///
    /// `v` must be String-tagged. Operating on a non-String value
    /// through this newtype is undefined per mruby's macro contract
    /// (the underlying `mrb_str_*` calls assume String layout).
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not yet
    /// migrated. Same conversion ladder as `Value::as_raw`.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_str_cat(mrb, self, p, len)` — append `bytes` to this string
    /// in place, the way Ruby's `String#<<` extends its receiver. The
    /// backing buffer may reallocate, but `self` keeps naming the same
    /// `RString`, so it stays usable after the call. Appending to a
    /// frozen string raises `FrozenError`; the call runs under
    /// `Mrb::protect`, so that surfaces as `Err` rather than
    /// long-jumping.
    #[inline]
    pub fn cat(self, mrb: &Mrb, bytes: &[u8]) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype
                // contract; `mrb` is alive inside the protect frame;
                // `bytes` is read-only and copied into the string's
                // buffer before the call returns. `mrb_str_cat` calls
                // `mrb_str_modify`, which raises `FrozenError` on a
                // frozen string — caught by `protect` into `Err`.
                unsafe {
                    sys::mrb_str_cat(
                        mrb.as_ptr(),
                        self.0.as_raw(),
                        bytes.as_ptr() as *const core::ffi::c_char,
                        bytes.len(),
                    );
                }
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, bytes);
            crate::not_linked()
        }
    }

    /// Borrow the raw bytes of this string. Routes through the
    /// `mrb_rstring_ptr` / `mrb_rstring_len` static-inline wrappers in
    /// `wrapper.h`, which expand the `RSTRING_PTR(s)` / `RSTRING_LEN(s)`
    /// macros inside the C compiler so the embed-vs-heap branch comes
    /// from mruby's own header rather than a Rust-side mirror.
    ///
    /// The returned slice points at storage owned by the mruby VM; the
    /// `&Mrb` borrow keeps the state alive for the slice's lifetime,
    /// but does not block GC or string mutation. Use `to_bytes` for an
    /// owned copy that outlives later calls.
    ///
    /// # Safety
    ///
    /// Caller must not invoke another mruby API that could free or move
    /// the string's backing buffer before consuming the slice.
    #[inline]
    pub unsafe fn as_bytes(self, _mrb: &Mrb) -> &[u8] {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is String-tagged by the newtype contract;
            // the wrapper-h inline helpers expand the RSTRING_PTR /
            // RSTRING_LEN macros against mruby's own headers.
            let ptr = unsafe { sys::mrb_rstring_ptr(self.0.as_raw()) } as *const u8;
            let len = unsafe { sys::mrb_rstring_len(self.0.as_raw()) } as usize;
            // SAFETY: ptr / len pair describes a buffer owned by mruby
            // and alive while the borrowed `&Mrb` outlives this slice.
            unsafe { core::slice::from_raw_parts(ptr, len) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Copy this string's bytes into an owned `Vec<u8>`. The bytes are
    /// copied out before returning, so — unlike `as_bytes` — the result
    /// needs no `&Mrb` lifetime anchor and outlives later mruby calls.
    /// Backs `FromValue for String` and `FromValue for Vec<u8>`.
    #[inline]
    pub fn to_bytes(self) -> Vec<u8> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb_rstring_ptr` / `mrb_rstring_len` read the RString
            // header without touching `mrb_state`, and the slice is
            // copied immediately, so no borrow escapes the VM-alive
            // window every `Value` already assumes.
            let bytes = unsafe {
                let ptr = sys::mrb_rstring_ptr(self.0.as_raw()) as *const u8;
                let len = sys::mrb_rstring_len(self.0.as_raw()) as usize;
                core::slice::from_raw_parts(ptr, len)
            };
            bytes.to_vec()
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `RSTRING_LEN(self)` — the number of bytes in this string, via the
    /// `mrb_rstring_len` shim (the macro expanded in the C compiler so
    /// the embed-vs-heap length read matches the linked archive's
    /// layout). It is a byte count, not a character count, and is never
    /// negative, so the result is returned as `usize`. Mirrors
    /// `Array::len`; cheaper than `to_bytes().len()`, which copies the
    /// buffer out first.
    #[inline]
    pub fn len(self) -> usize {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `RSTRING_LEN` reads only the string header.
            (unsafe { sys::mrb_rstring_len(self.0.as_raw()) }) as usize
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when the string holds no bytes.
    #[inline]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{Ccontext, Error, FromValue, Mrb, RString};

    #[test]
    fn cat_appends_bytes_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"foo");
        s.cat(&mrb, b"bar")
            .expect("appending to a mutable string succeeds");
        // The same handle now names the grown string — append mutated it
        // in place rather than producing a new object.
        assert_eq!(s.to_bytes(), b"foobar".to_vec());

        // Appending empty bytes leaves the receiver unchanged.
        s.cat(&mrb, b"").expect("appending nothing succeeds");
        assert_eq!(s.to_bytes(), b"foobar".to_vec());
    }

    #[test]
    fn cat_surfaces_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

        // A frozen String still carries the String tag, so the downcast
        // holds, but appending to it raises FrozenError — which protect
        // catches into Err rather than long-jumping.
        let frozen = RString::from_value(cxt.load_nstring(b"'fixed'.freeze"))
            .expect("a frozen String literal is String-tagged");
        assert!(
            mrb.pending_exc().is_nil(),
            "freezing the string must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        let result = frozen.cat(&mrb, b"more");
        assert!(matches!(result, Err(Error::Exception(_))));
    }

    #[test]
    fn len_and_is_empty_track_the_byte_count() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let empty = mrb.str_new(b"");
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        // The count is bytes, not characters: a 2-byte UTF-8 codepoint
        // contributes its bytes, so "héllo" measures 6, not 5.
        let s = mrb.str_new("héllo".as_bytes());
        assert_eq!(s.len(), 6);
        assert!(!s.is_empty());
    }

    #[test]
    fn to_bytes_copies_arbitrary_bytes() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Binary bytes survive the owned copy — `to_bytes` does not
        // require valid UTF-8.
        let s = mrb.str_new(&[0xff, 0x00, 0xfe]);
        assert_eq!(s.to_bytes(), vec![0xff, 0x00, 0xfe]);
    }
}
