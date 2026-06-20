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
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb` is alive; `mrb_str_substr` clamps the range and reads
            // only the byte buffer, returning a fresh String or `nil`.
            let v = Value::from_raw(unsafe {
                sys::mrb_str_substr(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    beg as sys::mrb_int,
                    len as sys::mrb_int,
                )
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
            // SAFETY: `self` is String-tagged by the newtype contract;
            // `mrb` is alive; `needle` is read-only and only scanned for
            // its `len` bytes. `mrb_str_index` does a pure `mrb_memsearch`
            // over the byte buffers, returning the byte index or -1.
            let pos = unsafe {
                sys::mrb_str_index(
                    mrb.as_ptr(),
                    self.0.as_raw(),
                    needle.as_ptr() as *const core::ffi::c_char,
                    needle.len() as sys::mrb_int,
                    offset as sys::mrb_int,
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
    /// string, or a `base` outside that domain — raises `ArgumentError`; the
    /// call runs under `Mrb::protect`, so that surfaces as `Err` rather than
    /// long-jumping.
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
    /// / `0b` / `0o` prefix; a `base` outside that domain is the one input the
    /// lenient parse cannot interpret and raises `ArgumentError`, which the
    /// surrounding `Mrb::protect` surfaces as `Err` rather than long-jumping.
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
    fn cat_appends_a_static_literal_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A `b"..."` literal is a `&'static [u8]`, so `cat` reaches what
        // C's `mrb_str_cat_lit(mrb, str, lit)` does — the literal-append path.
        let s = mrb.str_new(b"foo");
        s.cat(&mrb, b"bar").expect("appending a literal succeeds");
        assert_eq!(s.to_bytes(), b"foobar".to_vec());
    }

    #[test]
    fn cat_str_appends_another_string_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"foo");
        let tail = mrb.str_new(b"bar");
        s.cat_str(&mrb, tail)
            .expect("appending a string to a mutable string succeeds");
        // The receiver grew in place; the source is untouched.
        assert_eq!(s.to_bytes(), b"foobar".to_vec());
        assert_eq!(tail.to_bytes(), b"bar".to_vec());

        // Self-append doubles the receiver — the source snapshot is taken
        // before the buffer grows.
        s.cat_str(&mrb, s).expect("self-append succeeds");
        assert_eq!(s.to_bytes(), b"foobarfoobar".to_vec());
    }

    #[test]
    fn cat_str_surfaces_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

        let frozen = RString::from_value(cxt.load_nstring(b"'fixed'.freeze"))
            .expect("a frozen String literal is String-tagged");
        assert!(
            mrb.pending_exc().is_nil(),
            "freezing the string must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        let result = frozen.cat_str(&mrb, mrb.str_new(b"more"));
        assert!(matches!(result, Err(Error::Exception(_))));
    }

    #[test]
    fn cat_cstr_appends_a_c_string_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"foo");
        s.cat_cstr(&mrb, c"bar")
            .expect("appending a C string to a mutable string succeeds");
        // The same handle now names the grown string — the bytes up to the
        // terminating NUL were appended in place.
        assert_eq!(s.to_bytes(), b"foobar".to_vec());

        // Appending an empty C string leaves the receiver unchanged.
        s.cat_cstr(&mrb, c"").expect("appending nothing succeeds");
        assert_eq!(s.to_bytes(), b"foobar".to_vec());
    }

    #[test]
    fn cat_cstr_surfaces_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

        let frozen = RString::from_value(cxt.load_nstring(b"'fixed'.freeze"))
            .expect("a frozen String literal is String-tagged");
        let result = frozen.cat_cstr(&mrb, c"more");
        assert!(matches!(result, Err(Error::Exception(_))));
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
    fn dup_copies_into_an_independent_string() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"orig");
        let copy = s.dup(&mrb);

        // dup is an independent object: appending to the original leaves
        // the copy untouched.
        s.cat(&mrb, b"+more").expect("append succeeds");
        assert_eq!(copy.to_bytes(), b"orig".to_vec());
        assert_eq!(s.to_bytes(), b"orig+more".to_vec());
    }

    #[test]
    fn plus_concatenates_into_a_new_string_leaving_operands_unchanged() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let a = mrb.str_new(b"foo");
        let b = mrb.str_new(b"bar");
        // Capture both operands' bytes before the call to prove
        // non-mutation against the post-call reads.
        let a_before = a.to_bytes();
        let b_before = b.to_bytes();

        let joined = a.plus(&mrb, b);

        // The result is the concatenation of both operands.
        assert_eq!(joined.to_bytes(), b"foobar".to_vec());

        // Neither operand was mutated — plus builds a new string rather
        // than growing the receiver the way cat_str does.
        assert_eq!(a.to_bytes(), a_before);
        assert_eq!(b.to_bytes(), b_before);

        // The result is an independent object: growing it in place leaves
        // the receiver untouched.
        joined
            .cat(&mrb, b"!")
            .expect("appending to the result succeeds");
        assert_eq!(joined.to_bytes(), b"foobar!".to_vec());
        assert_eq!(a.to_bytes(), b"foo".to_vec());
    }

    #[test]
    fn cmp_orders_by_byte_content() {
        use core::cmp::Ordering;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let abc = mrb.str_new(b"abc");
        let abd = mrb.str_new(b"abd");
        let abc2 = mrb.str_new(b"abc");

        assert_eq!(abc.cmp(&mrb, abd), Ordering::Less);
        assert_eq!(abd.cmp(&mrb, abc), Ordering::Greater);
        assert_eq!(abc.cmp(&mrb, abc2), Ordering::Equal);

        // A prefix orders before the longer string it begins.
        let ab = mrb.str_new(b"ab");
        assert_eq!(ab.cmp(&mrb, abc), Ordering::Less);
    }

    #[test]
    fn eq_tests_byte_equality() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let abc = mrb.str_new(b"abc");
        let abc2 = mrb.str_new(b"abc");
        let abd = mrb.str_new(b"abd");

        // Same bytes in distinct objects are equal.
        assert!(abc.eq(&mrb, abc2));

        // Differing bytes of equal length are unequal.
        assert!(!abc.eq(&mrb, abd));

        // A prefix is unequal to the longer string it begins — the length
        // check rejects it before the byte compare.
        let ab = mrb.str_new(b"ab");
        assert!(!ab.eq(&mrb, abc));
    }

    #[test]
    fn intern_names_the_symbol_for_the_receiver_bytes() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // The interned symbol names the string's own bytes.
        let sym = mrb.str_new(b"flags").intern(&mrb);
        assert_eq!(sym.name(&mrb), Some("flags"));

        // Its id equals interning the same name directly — a wrong tag or
        // boxing in the unchecked wrap would diverge here.
        assert_eq!(sym.to_sym(), mrb.intern_cstr(c"flags"));
    }

    #[test]
    fn to_bytes_copies_arbitrary_bytes() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Binary bytes survive the owned copy — `to_bytes` does not
        // require valid UTF-8.
        let s = mrb.str_new(&[0xff, 0x00, 0xfe]);
        assert_eq!(s.to_bytes(), vec![0xff, 0x00, 0xfe]);
    }

    #[test]
    fn concat_coerces_a_non_string_argument_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A plain String argument appends like cat_str.
        let s = mrb.str_new(b"foo");
        s.concat(&mrb, mrb.str_new(b"bar").as_value())
            .expect("appending a string succeeds");
        assert_eq!(s.to_bytes(), b"foobar".to_vec());

        // A non-string argument is coerced before appending: an Integer
        // renders to its decimal text.
        s.concat(&mrb, crate::Value::from_int(&mrb, 42))
            .expect("appending a coerced integer succeeds");
        assert_eq!(s.to_bytes(), b"foobar42".to_vec());
    }

    #[test]
    fn concat_surfaces_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

        let frozen = RString::from_value(cxt.load_nstring(b"'fixed'.freeze"))
            .expect("a frozen String literal is String-tagged");
        let result = frozen.concat(&mrb, mrb.str_new(b"more").as_value());
        assert!(matches!(result, Err(Error::Exception(_))));
    }

    #[test]
    fn resize_truncates_and_extends_in_place() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Shrinking drops the tail; the same handle names the result.
        let s = mrb.str_new(b"Hello, world!");
        s.resize(&mrb, 5).expect("shrinking succeeds");
        assert_eq!(s.to_bytes(), b"Hello".to_vec());

        // Growing extends the length; the original prefix is preserved,
        // the new tail's contents are unspecified, so only the length is
        // asserted.
        s.resize(&mrb, 8).expect("growing succeeds");
        assert_eq!(s.len(), 8);
        assert_eq!(&s.to_bytes()[..5], b"Hello");
    }

    #[test]
    fn resize_surfaces_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

        let frozen = RString::from_value(cxt.load_nstring(b"'fixed'.freeze"))
            .expect("a frozen String literal is String-tagged");
        assert!(matches!(frozen.resize(&mrb, 2), Err(Error::Exception(_))));
    }

    #[test]
    fn to_cstr_yields_a_nul_terminated_view() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"hello");
        let cstr = s.to_cstr(&mrb).expect("a NUL-free string yields a CString");
        assert_eq!(cstr.to_bytes(), b"hello");
        // The view carries the terminating NUL the C boundary expects.
        assert_eq!(cstr.to_bytes_with_nul(), b"hello\0");
    }

    #[test]
    fn to_cstr_surfaces_an_embedded_nul_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A C string cannot carry an embedded NUL, so the read raises
        // ArgumentError, which protect catches into Err.
        let s = mrb.str_new(b"a\0b");
        assert!(matches!(s.to_cstr(&mrb), Err(Error::Exception(_))));
    }

    #[test]
    fn substr_reads_a_range_and_clamps_out_of_range() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"Hello, world!");

        // An in-range slice yields the substring.
        let he = s.substr(&mrb, 0, 2).expect("an in-range slice is Some");
        assert_eq!(he.to_bytes(), b"He".to_vec());

        // A negative beg counts from the end.
        let bang = s.substr(&mrb, -1, 1).expect("a tail slice is Some");
        assert_eq!(bang.to_bytes(), b"!".to_vec());

        // An over-long len clamps to the string's end.
        let tail = s.substr(&mrb, 7, 100).expect("an over-long len clamps");
        assert_eq!(tail.to_bytes(), b"world!".to_vec());

        // A beg past the end yields None, the way mruby returns nil.
        assert!(s.substr(&mrb, 100, 1).is_none());
    }

    #[test]
    fn index_finds_the_first_match_at_or_after_the_offset() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let s = mrb.str_new(b"hello, hello");

        // A present substring returns the byte index of its first match.
        assert_eq!(s.index(&mrb, b"llo", 0), Some(2));

        // An absent substring returns None.
        assert!(s.index(&mrb, b"xyz", 0).is_none());

        // The offset is respected: a match before it is skipped, and the
        // next match's index is reported.
        assert_eq!(s.index(&mrb, b"llo", 3), Some(9));

        // A negative offset counts from the end.
        assert_eq!(s.index(&mrb, b"hello", -5), Some(7));

        // An empty needle is found at the offset itself.
        assert_eq!(s.index(&mrb, b"", 4), Some(4));

        // An offset past the end finds nothing.
        assert!(s.index(&mrb, b"hello", 100).is_none());
    }

    #[test]
    fn to_i_parses_in_the_requested_base() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A clean decimal string parses to its value.
        assert_eq!(
            mrb.str_new(b"12345")
                .to_i(&mrb, 10)
                .expect("a decimal string parses"),
            12345
        );

        // The same digits read in base 16 take their hexadecimal value.
        assert_eq!(
            mrb.str_new(b"ff")
                .to_i(&mrb, 16)
                .expect("a hex string parses"),
            255
        );

        // Base 0 auto-detects a leading prefix.
        assert_eq!(
            mrb.str_new(b"0b101")
                .to_i(&mrb, 0)
                .expect("a prefixed string auto-detects its base"),
            5
        );
    }

    #[test]
    fn to_i_surfaces_invalid_input_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Trailing junk is rejected by the strict parse — unlike Ruby's
        // lenient String#to_i, which would stop at the first bad character.
        assert!(matches!(
            mrb.str_new(b"99 red balloons").to_i(&mrb, 10),
            Err(Error::Exception(_))
        ));

        // Bytes with no valid integer at all raise too.
        assert!(matches!(
            mrb.str_new(b"hello").to_i(&mrb, 10),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn to_inum_parses_leniently_without_raising_on_malformed_content() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A clean decimal string parses to its value.
        assert_eq!(
            mrb.str_new(b"12345")
                .to_inum(&mrb, 10)
                .expect("a decimal string parses"),
            12345
        );

        // Trailing junk is ignored — unlike strict to_i, the lenient parse
        // consumes the leading integer and stops, the way Ruby's String#to_i
        // does.
        assert_eq!(
            mrb.str_new(b"12abc")
                .to_inum(&mrb, 10)
                .expect("a malformed-prefix string reads its leading integer"),
            12
        );

        // Bytes with no integer at the start yield 0 rather than an Err.
        assert_eq!(
            mrb.str_new(b"hello")
                .to_inum(&mrb, 10)
                .expect("non-numeric bytes read 0"),
            0
        );

        // A non-decimal base parses in that radix.
        assert_eq!(
            mrb.str_new(b"ff")
                .to_inum(&mrb, 16)
                .expect("a hex string parses"),
            255
        );
    }

    #[test]
    fn to_inum_surfaces_an_illegal_radix_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A radix outside the 2-through-36 / 0-prefix domain is the one input
        // the lenient parse cannot interpret, so it raises ArgumentError even
        // with strict checking off — protect catches it into Err.
        assert!(matches!(
            mrb.str_new(b"10").to_inum(&mrb, 99),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn to_f_parses_a_float_string() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A clean float string parses to its value.
        assert_eq!(
            mrb.str_new(b"2.5")
                .to_f(&mrb)
                .expect("a float string parses"),
            2.5
        );

        // Scientific notation parses too.
        assert_eq!(
            mrb.str_new(b"123.45e1")
                .to_f(&mrb)
                .expect("a scientific-notation string parses"),
            1234.5
        );
    }

    #[test]
    fn to_f_surfaces_invalid_input_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Trailing junk is rejected by the strict parse — unlike Ruby's
        // lenient String#to_f, which would ignore the trailing characters.
        assert!(matches!(
            mrb.str_new(b"45.67 degrees").to_f(&mrb),
            Err(Error::Exception(_))
        ));

        // Bytes with no valid float at all raise too.
        assert!(matches!(
            mrb.str_new(b"thx1138").to_f(&mrb),
            Err(Error::Exception(_))
        ));
    }
}
