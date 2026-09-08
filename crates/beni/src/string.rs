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

    /// `mrb_str_cat_str(mrb, self, other)` — append `other`'s bytes to
    /// this string in place, the `RString` counterpart of `cat`, the way
    /// Ruby's `String#<<` extends its receiver with another string. The
    /// backing buffer may reallocate, but `self` keeps naming the same
    /// `RString`. Appending to a frozen string raises `FrozenError`; the
    /// call runs under `Mrb::protect`, so that surfaces as `Err` rather
    /// than long-jumping. Self-append (`s.cat_str(&mrb, s)`) is handled
    /// by `mrb_str_cat_str`, which snapshots the source before growing.
    #[inline]
    pub fn cat_str(self, mrb: &Mrb, other: RString) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` and `other` are String-tagged by the
                // newtype contract; `mrb` is alive inside the protect
                // frame. `mrb_str_cat_str` calls `mrb_str_modify`, which
                // raises `FrozenError` on a frozen receiver — caught by
                // `protect` into `Err`.
                unsafe {
                    sys::mrb_str_cat_str(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw());
                }
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_str_cat_cstr(mrb, self, ptr)` — append a NUL-terminated C
    /// string's bytes to this string in place, its content up to the
    /// terminating NUL, the C-boundary counterpart of `cat`, the way
    /// Ruby's `String#<<` extends its receiver. The backing buffer may
    /// reallocate, but `self` keeps naming the same `RString`. Appending
    /// to a frozen string raises `FrozenError`; the call runs under
    /// `Mrb::protect`, so that surfaces as `Err` rather than long-jumping.
    #[inline]
    pub fn cat_cstr(self, mrb: &Mrb, s: &core::ffi::CStr) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype contract;
                // `mrb` is alive inside the protect frame; `s` is a
                // NUL-terminated buffer read up to its terminator and
                // copied into the string before the call returns.
                // `mrb_str_cat_cstr` calls `mrb_str_modify`, which raises
                // `FrozenError` on a frozen receiver — caught by `protect`
                // into `Err`.
                unsafe {
                    sys::mrb_str_cat_cstr(mrb.as_ptr(), self.0.as_raw(), s.as_ptr());
                }
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, s);
            crate::not_linked()
        }
    }

    /// `mrb_str_concat(mrb, self, other)` — append `other` coerced to a
    /// String, the dispatching counterpart of `cat_str`, the way Ruby's
    /// `String#concat` accepts a non-string argument. A non-string
    /// `other` runs the same coercion as `Value::obj_as_string` (a
    /// Symbol/Integer/Class renders directly, anything else dispatches
    /// `to_s`), which may raise; appending to a frozen receiver raises
    /// `FrozenError`. The call runs under `Mrb::protect`, so either
    /// surfaces as `Err` rather than long-jumping.
    #[inline]
    pub fn concat(self, mrb: &Mrb, other: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype
                // contract; `mrb` is alive inside the protect frame;
                // `other` shares the VM. `mrb_str_concat` coerces
                // `other` to a String (dispatching `to_s` where needed)
                // and calls `mrb_str_modify`; a frozen receiver or a
                // raising coercion long-jumps — caught by `protect`.
                unsafe {
                    sys::mrb_str_concat(mrb.as_ptr(), self.0.as_raw(), other.as_raw());
                }
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_str_resize(mrb, self, len)` — set this string's byte length
    /// in place: shrinking drops the tail, growing leaves the new
    /// trailing bytes undefined. The same handle keeps naming the
    /// resized string. Resizing a frozen string raises `FrozenError`,
    /// and a `len` mruby's integer cannot hold (including a length at its
    /// maximum) raises `ArgumentError`; the call runs under
    /// `Mrb::protect`, so either surfaces as `Err` rather than
    /// long-jumping.
    #[inline]
    pub fn resize(self, mrb: &Mrb, len: usize) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                match sys::mrb_int::try_from(len) {
                    // SAFETY: `self` is String-tagged by the newtype
                    // contract; `mrb` is alive inside the protect frame.
                    // `mrb_str_resize` calls `mrb_str_modify` (raises
                    // `FrozenError` on a frozen receiver) and
                    // `str_check_length` (raises `ArgumentError` on a
                    // length at the integer maximum) — both long-jump,
                    // caught by `protect`.
                    Ok(len) => unsafe {
                        sys::mrb_str_resize(mrb.as_ptr(), self.0.as_raw(), len);
                    },
                    // A `usize` past mruby's integer range can never be a
                    // valid string length; raise the same `ArgumentError`
                    // mruby raises for an overflowed length so the caller
                    // sees one error shape regardless of where the bound
                    // is hit.
                    Err(_) => {
                        // SAFETY: `mrb` is alive; `E_ARGUMENT_ERROR` is a
                        // core class so the lookup cannot fail;
                        // `mrb_raise` long-jumps to the protect frame.
                        unsafe {
                            let argerr =
                                sys::mrb_class_get(mrb.as_ptr(), c"ArgumentError".as_ptr());
                            sys::mrb_raise(mrb.as_ptr(), argerr, c"string size too large".as_ptr());
                        }
                    }
                }
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, len);
            crate::not_linked()
        }
    }

    /// `mrb_str_substr(mrb, self, beg, len)` — a substring by character
    /// range, Ruby's `String#[beg, len]`. A negative `beg` counts from
    /// the end and an over-long `len` clamps to the string; a range that
    /// starts past the end yields `None`, matching the `nil` mruby
    /// returns. It allocates a fresh String and dispatches nothing, so it
    /// never raises. Mirrors magnus's `RString` substring read.
    #[inline]
    pub fn substr(self, mrb: &Mrb, beg: i64, len: i64) -> Option<RString> {
        #[cfg(mruby_linked)]
        {
            // An offset or length outside the archive's `mrb_int` width
            // names no position; saturate it to the nearest bound so the
            // clamp still sees "past the beginning" / "past the end" rather
            // than a truncated value landing on a wrong in-range position.
            let beg = sys::mrb_int::try_from(beg).unwrap_or(if beg < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            let len = sys::mrb_int::try_from(len).unwrap_or(if len < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb` is alive; `mrb_str_substr` clamps the range and reads
            // only the byte buffer, returning a fresh String or `nil`.
            let v = Value::from_raw(unsafe {
                sys::mrb_str_substr(mrb.as_ptr(), self.0.as_raw(), beg, len)
            });
            if v.is_nil() {
                None
            } else {
                // SAFETY: a non-nil `mrb_str_substr` result is
                // String-tagged.
                Some(unsafe { RString::from_value_unchecked(v) })
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, beg, len);
            crate::not_linked()
        }
    }

    /// `mrb_str_index(mrb, self, needle, len, offset)` — search for
    /// `needle`'s bytes, yielding the byte index of the first match at or
    /// after `offset`, or `None` when the substring is absent, the byte
    /// counterpart of Ruby's `String#index`. A negative `offset` counts
    /// from the end, an `offset` past the end finds nothing, and an empty
    /// `needle` is found at `offset` itself. It scans the byte buffers and
    /// dispatches nothing, so it never raises; mruby's -1 maps to `None`.
    /// The byte-index sibling of the character-range `substr` read.
    #[inline]
    pub fn index(self, mrb: &Mrb, needle: &[u8], offset: i64) -> Option<usize> {
        #[cfg(mruby_linked)]
        {
            // An offset outside the archive's `mrb_int` width names no
            // position; saturate it to the nearest bound so the scan still
            // sees "past the beginning" / "past the end" rather than a
            // truncated value landing on a wrong in-range offset. The needle
            // length is non-negative; a length past `mrb_int::MAX` cannot fit
            // before the end either, so it saturates upward to stay "not
            // found".
            let offset = sys::mrb_int::try_from(offset).unwrap_or(if offset < 0 {
                sys::mrb_int::MIN
            } else {
                sys::mrb_int::MAX
            });
            let slen = sys::mrb_int::try_from(needle.len()).unwrap_or(sys::mrb_int::MAX);
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb` is alive; `needle` is read-only and only scanned for
            // its `len` bytes. `mrb_str_index` does a pure `mrb_memsearch`
            // over the byte buffers, returning the byte index or -1.
            let pos = unsafe {
                sys::mrb_str_index(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    needle.as_ptr() as *const core::ffi::c_char,
                    slen,
                    offset,
                )
            };
            if pos < 0 {
                None
            } else {
                Some(pos as usize)
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, needle, offset);
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

    /// `mrb_str_dup(mrb, self)` — a copy with its own buffer, Ruby's
    /// `String#dup`. It does not mutate the receiver, so it never fails.
    /// Mirrors `Array::dup` / `Hash::dup`; mruby has no copy-on-write
    /// share here, so the bytes are copied outright.
    #[inline]
    pub fn dup(self, mrb: &Mrb) -> RString {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb_str_dup` returns a fresh String-tagged value, so the
            // unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_dup(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_str_plus(mrb, self, other)` — concatenate into a fresh String
    /// holding both operands' bytes, Ruby's `String#+`. It allocates a new
    /// string and copies the bytes, mutating neither operand — the
    /// non-mutating counterpart of `cat_str`, which grows its receiver in
    /// place. With both sides String-tagged it dispatches nothing and never
    /// raises, so it returns the new `RString` directly. Mirrors `dup`.
    #[inline]
    pub fn plus(self, mrb: &Mrb, other: RString) -> RString {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` and `other` are String-tagged by the newtype
            // contract and share the VM; `mrb_str_plus` reads only their
            // byte buffers and returns a fresh String-tagged value, so the
            // unchecked wrap is sound.
            unsafe {
                RString::from_value_unchecked(Value::from_raw(sys::mrb_str_plus(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    other.0.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_str_cmp(mrb, self, other)` — order this string against
    /// `other` by byte content, Ruby's `String#<=>`. A pure `memcmp`
    /// that dispatches nothing, so it never raises: a shared prefix
    /// orders the shorter string first, and equal bytes of equal length
    /// compare `Equal`. The `RString` type on both sides guarantees the
    /// String layout `mrb_str_cmp` assumes. Mirrors magnus's
    /// `RString::cmp`.
    #[inline]
    pub fn cmp(self, mrb: &Mrb, other: RString) -> core::cmp::Ordering {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` and `other` are String-tagged by the newtype
            // contract and share the VM; `mrb_str_cmp` reads only their
            // byte buffers and returns -1 / 0 / 1.
            let ord = unsafe { sys::mrb_str_cmp(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) };
            ord.cmp(&0)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_str_equal(mrb, self, other)` — TRUE when the two strings hold the
    /// same bytes, Ruby's `String#==`. A length check then a `memcmp` that
    /// dispatches nothing, so it never raises. The `RString` type on both sides
    /// guarantees the String layout `mrb_str_equal` assumes, so its non-String
    /// guard never fires — this is total byte equality. The equality sibling of
    /// the ordering `cmp`.
    #[inline]
    pub fn eq(self, mrb: &Mrb, other: RString) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` and `other` are String-tagged by the newtype
            // contract and share the VM; `mrb_str_equal` reads only their byte
            // buffers and returns a boolean.
            unsafe { sys::mrb_str_equal(mrb.as_ptr(), self.0.as_raw(), other.0.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_str_intern(mrb, self)` — the typed `Symbol` naming this
    /// string's own bytes, Ruby's `String#intern`, creating the symbol
    /// when it does not yet exist. It interns the receiver's bytes
    /// directly (`mrb_symbol_value(mrb_intern_str(..))`), dispatching
    /// nothing and never raising. Distinct from `Value::to_sym`, which
    /// coerces an arbitrary value and can raise.
    #[inline]
    pub fn intern(self, mrb: &Mrb) -> crate::Symbol {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is String-tagged by the newtype contract and
            // shares the VM; `mrb_str_intern` reads its bytes and returns a
            // Symbol-tagged value, so the unchecked wrap is sound.
            unsafe {
                crate::Symbol::from_value_unchecked(Value::from_raw(sys::mrb_str_intern(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                )))
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_string_cstr(mrb, self)` — the bytes as an owned, NUL-terminated
    /// `CString` for a C boundary. A C string cannot carry an embedded NUL,
    /// so this read is fallible: an embedded NUL raises `ArgumentError`, and
    /// the call runs under `Mrb::protect` so that surfaces as `Err` rather
    /// than long-jumping. magnus has no direct C-string accessor, so this
    /// anchors on mruby's own `mrb_string_cstr`.
    #[inline]
    pub fn to_cstr(self, mrb: &Mrb) -> Result<std::ffi::CString, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype contract;
                // `mrb` is alive inside the protect frame. `mrb_string_cstr`
                // NUL-terminates the buffer in place, raising `ArgumentError`
                // on an embedded NUL — caught by `protect` into `Err`. The
                // returned pointer is discarded; the CString is rebuilt from
                // the receiver's now-NUL-free bytes after protect returns.
                unsafe {
                    sys::mrb_string_cstr(mrb.as_ptr(), self.0.as_raw());
                }
                Value::nil()
            })?;
            // On the success path `mrb_string_cstr` proved the bytes hold no
            // NUL, so the CString build cannot fail.
            Ok(std::ffi::CString::new(self.to_bytes())
                .expect("mrb_string_cstr rejected any embedded NUL"))
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_str_to_integer(mrb, self, base, TRUE)` — parse the bytes to an
    /// integer in `base`, the strict counterpart of Ruby's lenient
    /// `String#to_i`. The `base` is 2 through 36, or 0 to auto-detect a
    /// leading `0x` / `0b` / `0o` prefix. With strict checking on, any input
    /// that is not a clean integer in the base — trailing junk, an empty
    /// string, or a positive `base` outside 2 through 36 — raises
    /// `ArgumentError`; the call runs under `Mrb::protect`, so that surfaces
    /// as `Err` rather than long-jumping. A negative `base` is not an error:
    /// `-n` aliases the radix `n` with prefix detection disabled.
    #[inline]
    pub fn to_i(self, mrb: &Mrb, base: i32) -> Result<sys::mrb_int, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype contract;
                // `mrb` is alive inside the protect frame. `mrb_str_to_integer`
                // with `badcheck` TRUE raises `ArgumentError` on any input that
                // is not a clean integer in the base — caught by `protect` into
                // `Err`. On success it returns an Integer-tagged value.
                Value::from_raw(unsafe {
                    sys::mrb_str_to_integer(
                        mrb.as_ptr(),
                        self.0.as_raw(),
                        base as sys::mrb_int,
                        true,
                    )
                })
            })
            // SAFETY: a successful `mrb_str_to_integer` returns an
            // Integer-tagged value, so the unbox accepts it.
            .map(|v| unsafe { v.unbox_integer() })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, base);
            crate::not_linked()
        }
    }

    /// `mrb_str_to_inum(mrb, self, base, FALSE)` — parse the bytes to an
    /// integer in `base` the lenient way Ruby's `String#to_i` itself does, the
    /// best-effort counterpart of the strict `to_i`. It consumes the leading
    /// integer and ignores any trailing characters, and yields `0` when no
    /// integer begins the bytes rather than rejecting them — so `"12abc"` reads
    /// `12`, `"hello"` and `""` read `0`, and malformed content never surfaces
    /// an `Err`. The `base` is 2 through 36, or 0 to auto-detect a leading `0x`
    /// / `0b` / `0o` prefix; a positive `base` outside 2 through 36 is the one
    /// input the lenient parse cannot interpret and raises `ArgumentError`,
    /// which the surrounding `Mrb::protect` surfaces as `Err` rather than
    /// long-jumping. A negative `base` is not an error: `-n` aliases the radix
    /// `n` with prefix detection disabled.
    #[inline]
    pub fn to_inum(self, mrb: &Mrb, base: i32) -> Result<sys::mrb_int, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype contract;
                // `mrb` is alive inside the protect frame. `mrb_str_to_integer`
                // with `badcheck` FALSE never raises on malformed content — it
                // returns the leading integer or 0 — but still raises
                // `ArgumentError` on an out-of-domain radix, caught by `protect`
                // into `Err`. On success it returns an Integer-tagged value.
                Value::from_raw(unsafe {
                    sys::mrb_str_to_integer(
                        mrb.as_ptr(),
                        self.0.as_raw(),
                        base as sys::mrb_int,
                        false,
                    )
                })
            })
            // SAFETY: a successful `mrb_str_to_integer` returns an
            // Integer-tagged value, so the unbox accepts it.
            .map(|v| unsafe { v.unbox_integer() })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, base);
            crate::not_linked()
        }
    }

    /// `mrb_str_to_dbl(mrb, self, TRUE)` — parse the bytes to a float, the
    /// strict counterpart of Ruby's lenient `String#to_f`. With strict
    /// checking on, any input that is not a clean float — trailing junk or
    /// bytes with no valid float at all — raises `ArgumentError`; the call
    /// runs under `Mrb::protect`, so that surfaces as `Err` rather than
    /// long-jumping. The `to_i` sibling for the integer parse.
    #[inline]
    pub fn to_f(self, mrb: &Mrb) -> Result<sys::mrb_float, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `self` is String-tagged by the newtype contract;
                // `mrb` is alive inside the protect frame. `mrb_str_to_dbl`
                // with `badcheck` TRUE raises `ArgumentError` on any input
                // that is not a clean float — caught by `protect` into `Err`.
                // The C `double` is boxed into a Float value so it rides the
                // protect frame's `Value` return.
                let d = unsafe { sys::mrb_str_to_dbl(mrb.as_ptr(), self.0.as_raw(), true) };
                Value::from_float(mrb, d)
            })
            // SAFETY: the `Ok` value was boxed by `Value::from_float`, so
            // the unbox accepts it.
            .map(|v| unsafe { v.unbox_float() })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}
