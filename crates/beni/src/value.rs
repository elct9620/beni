//! Typed `Value` newtype around the raw `mrb_value` FFI word-box.
//!
//! ## Why a newtype
//!
//! Three reasons stack here:
//!
//! 1. **Orphan rule** — `mrb_value` is declared in `beni-sys` so the
//!    FFI ABI stays accessible to other crates, which means no crate
//!    downstream of it can attach inherent methods. Wrapping the type
//!    here removes the extension-trait + per-call-site `use`
//!    workaround that restriction otherwise forces.
//! 2. **API surface clarity** — methods that operate on values
//!    (classname, to_string, predicates, unboxers) become inherent
//!    on `Value`, so the call shape is `val.classname(mrb)` rather
//!    than splatting raw FFI calls.
//! 3. **Migration anchor** — typed `Value` is the natural place to
//!    later attach typed variants (`MString`, `MArray`, `MHash`) and
//!    convert between them. Today no typed variants exist; the
//!    newtype is the floor on which they can be added.
//!
//! ## ABI guarantee
//!
//! `Value` is `#[repr(transparent)]` over `mrb_value`. Under beni's
//! pinned word-boxing config `mrb_value` is a single machine word
//! (4 bytes on wasm32, 8 on 64-bit hosts); `Value` shares that
//! layout and the C ABI. This matters at the `mrb_func_t` boundary:
//! a bridge declared with `Value` parameters and return type
//! produces the same function signature as one declared with
//! `mrb_value`. Round-tripping through `Value::from_raw` /
//! `Value::into_raw` is therefore a no-op at the codegen level.
//!
//! ## What lives next to `Value` here
//!
//!   * The `cstr!` macro and `cstr_ptr` helper — generic
//!     NUL-terminated `*const c_char` plumbing; unchanged across
//!     the `Value` introduction.
//!   * The `Immediates` cache — `nil` / `true` / `false`
//!     `mrb_value` snapshots captured once via the layout-safe C
//!     shims, exposed through `Value::nil` / `Value::true_` /
//!     `Value::false_`.

use beni_sys as sys;

use crate::{Error, Mrb, RClass};
#[cfg(mruby_linked)]
use crate::{FromValue, RString};

/// Compile-time NUL-terminated C-string literal pointer.
///
/// `cstr!("name")` expands to `concat!("name", "\0").as_ptr() as *const c_char`,
/// avoiding the noisy hand-written `b"name\0".as_ptr() as *const core::ffi::c_char`
/// pattern at every FFI call site.
#[macro_export]
macro_rules! cstr {
    ($s:expr) => {
        concat!($s, "\0").as_ptr() as *const core::ffi::c_char
    };
}

/// Coerce a NUL-terminated byte slice to `*const c_char`. Used for the
/// top-of-file `const X: &[u8] = b"...\0"` declarations that already
/// carry their NUL terminator — `cstr_ptr(CLASS_NAME)` reads cleaner
/// than `CLASS_NAME.as_ptr() as *const core::ffi::c_char`.
///
/// The caller must guarantee `b` ends with `0u8` — debug builds assert.
#[inline]
pub const fn cstr_ptr(b: &[u8]) -> *const core::ffi::c_char {
    debug_assert!(!b.is_empty());
    debug_assert!(b[b.len() - 1] == 0);
    b.as_ptr() as *const core::ffi::c_char
}

// --------------------------------------------------------------------
// Immediates cache.
// --------------------------------------------------------------------
//
// `mrb_nil_value()` / `mrb_true_value()` / `mrb_false_value()` are
// config-level constants under mruby's word-box configuration — they
// are decided at libmruby build time and do not vary across
// `mrb_state` instances. Capturing them once via the C shims sidesteps
// a cross-FFI call every time a hot path wants `nil` / `true` /
// `false`.

#[cfg(mruby_linked)]
struct Immediates {
    qnil: sys::mrb_value,
    qtrue: sys::mrb_value,
    qfalse: sys::mrb_value,
}

// SAFETY: `mrb_value` under word boxing is a `#[repr(C)]` struct
// holding a single integer word — plain old data with no interior
// mutability. `Immediates` therefore shares only `Copy` snapshots,
// which is sound to read from any thread.
#[cfg(mruby_linked)]
unsafe impl Sync for Immediates {}

#[cfg(mruby_linked)]
static IMMEDIATES: std::sync::OnceLock<Immediates> = std::sync::OnceLock::new();

#[cfg(mruby_linked)]
impl Immediates {
    /// Return the cached snapshot, capturing it on first call.
    fn get() -> &'static Immediates {
        IMMEDIATES.get_or_init(|| {
            // SAFETY: the three helpers are mruby's own
            // `mrb_nil_value` / `mrb_true_value` / `mrb_false_value`
            // (`MRB_INLINE`s reached through bindgen's static-fn
            // trampolines). They do not touch `mrb_state`.
            unsafe {
                Immediates {
                    qnil: sys::mrb_nil_value(),
                    qtrue: sys::mrb_true_value(),
                    qfalse: sys::mrb_false_value(),
                }
            }
        })
    }
}

// --------------------------------------------------------------------
// Value newtype.
// --------------------------------------------------------------------

/// Typed handle on a single mruby value. `#[repr(transparent)]` over
/// `mrb_value` so the C ABI is preserved.
///
/// Construct via `Value::from_raw` (at FFI boundaries),
/// `Value::nil` / `Value::true_` / `Value::false_` (immediates),
/// or `Value::from_int` / `Value::from_float` (numeric factories).
/// Round-trip back to the raw type via `Value::as_raw` /
/// `Value::into_raw` when calling raw FFI that has not yet been
/// migrated.
///
/// ## What is intentionally NOT here
///
/// No typed variants (`MString` / `MArray` / `MHash`). The
/// `mrb_value` word-box ABI is small enough that we keep passing
/// `Value` directly through the codebase. Typed variants can land
/// later as `pub struct MString(Value)` newtypes if the call sites
/// justify them.
///
/// ## Cross-target availability
///
/// The whole `Value` surface compiles on every target and in
/// placeholder mode alike. Pure layout operations (`from_raw` /
/// `as_raw` / `into_raw` / `zeroed`) work everywhere; methods that
/// talk to mruby keep their signatures in placeholder mode but
/// divert to `crate::not_linked` because the mruby symbols they
/// wrap are not linked. The ABI invariant checks
/// (`value::tests::value_shares_abi_with_mrb_value` here,
/// `tests::typed_mrb_func_t_coerces_from_value_bridge` at the crate
/// root) pin the `#[repr(transparent)]` contract that
/// `Class::define_method`'s `mem::transmute` depends on.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub(crate) sys::mrb_value);

// Manual and deliberately opaque: the boxed payload is meaningless
// without the VM that produced it (and its layout varies by boxing
// config), so the debug form identifies the type without pretending
// to render the value. Lets containers like `Error` derive `Debug`.
impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Value").finish_non_exhaustive()
    }
}

impl Value {
    /// Wrap a raw `mrb_value` produced by FFI. The most common
    /// caller is a bridge function pointer receiving the receiver
    /// from mruby.
    #[inline]
    pub const fn from_raw(v: sys::mrb_value) -> Self {
        Self(v)
    }

    /// Borrow the inner `mrb_value` for raw FFI calls. Use this when
    /// passing the value through an as-yet-unmigrated `extern "C" fn`
    /// parameter. The wrapper itself stays usable after the borrow
    /// (`Value: Copy`).
    #[inline]
    pub const fn as_raw(self) -> sys::mrb_value {
        self.0
    }

    /// Consume and return the inner `mrb_value`. Identical to
    /// `Value::as_raw` semantically — `Value: Copy` makes the move
    /// vs. borrow distinction immaterial — but reads cleaner at the
    /// final return statement of a bridge function.
    #[inline]
    pub const fn into_raw(self) -> sys::mrb_value {
        self.0
    }

    /// All-zero `Value`. Under beni's pinned word-boxing
    /// configuration this matches `mrb_nil_value()` (MRB_Qnil = 0),
    /// but callers that need a guaranteed nil should prefer
    /// `Value::nil` which reads through the mruby shim. The
    /// zeroed form exists for out-parameter initialization
    /// (`mrb_get_args` writes to it).
    #[inline]
    pub fn zeroed() -> Self {
        Self(sys::mrb_value::zeroed())
    }
}

impl Value {
    /// Canonical mruby `nil`. Reads through the process-wide
    /// `Immediates` cache; capture is lazy and one-shot.
    #[inline]
    pub fn nil() -> Self {
        #[cfg(mruby_linked)]
        {
            Self(Immediates::get().qnil)
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Canonical mruby `true`. See `Value::nil`.
    #[inline]
    pub fn true_() -> Self {
        #[cfg(mruby_linked)]
        {
            Self(Immediates::get().qtrue)
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Canonical mruby `false`. See `Value::nil`.
    #[inline]
    pub fn false_() -> Self {
        #[cfg(mruby_linked)]
        {
            Self(Immediates::get().qfalse)
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_int_value(mrb, n)` — construct an mruby Integer from `n`,
    /// via mruby's own boxing-agnostic `MRB_INLINE` constructor
    /// (reached through bindgen's static-fn trampoline, compiled with
    /// the same defines as the linked archive). `sys::mrb_int` follows
    /// the archive's configured width — 64-bit under mruby's 64-bit
    /// platform default, 32-bit under `MRB_INT32` or on wasm32.
    #[inline]
    pub fn from_int(mrb: &Mrb, n: sys::mrb_int) -> Self {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the `&Mrb` borrow.
            Self(unsafe { sys::mrb_int_value(mrb.as_ptr(), n) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, n);
            crate::not_linked()
        }
    }

    /// `mrb_float_value(mrb, f)` — construct an mruby Float from `f`,
    /// via mruby's boxing-agnostic `MRB_INLINE` constructor (same
    /// trampoline route as `Value::from_int`).
    #[inline]
    pub fn from_float(mrb: &Mrb, f: sys::mrb_float) -> Self {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the `&Mrb` borrow.
            Self(unsafe { sys::mrb_float_value(mrb.as_ptr(), f) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, f);
            crate::not_linked()
        }
    }

    /// Render this Integer value to a new `RString` in `base`, the way
    /// Ruby's `Integer#to_s(base)` does — `12345` to `"3039"` in base 16.
    /// `base` is 2 through 36; a base outside that domain raises
    /// `ArgumentError`. The render guards its receiver on the Integer tag
    /// rather than trusting it, raising `TypeError` for any other tag so a
    /// non-Integer never reaches `mrb_integer_to_str`'s unchecked unbox.
    /// Both raises run under `Mrb::protect`, so either surfaces as `Err`
    /// rather than long-jumping. magnus offers no direct radix render, so
    /// this anchors on mruby's own `mrb_integer_to_str`.
    #[inline]
    pub fn int_to_str(self, mrb: &Mrb, base: i32) -> Result<crate::RString, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                if !self.is_integer() {
                    // SAFETY: `mrb` is alive inside the protect frame;
                    // `TypeError` is a core class so the lookup cannot fail;
                    // `mrb_raise` long-jumps to the protect frame. The guard
                    // is strict — a Float is rejected, not coerced — because
                    // `mrb_integer_to_str` unboxes its receiver without a tag
                    // check.
                    unsafe {
                        let typeerr = sys::mrb_class_get(mrb.as_ptr(), c"TypeError".as_ptr());
                        sys::mrb_raise(
                            mrb.as_ptr(),
                            typeerr,
                            c"no implicit conversion to Integer".as_ptr(),
                        );
                    }
                }
                // SAFETY: `self` is Integer-tagged past the guard; `mrb` is
                // alive inside the protect frame. `mrb_integer_to_str` raises
                // `ArgumentError` on a base outside 2 through 36 — caught by
                // `protect` into `Err` — and otherwise returns a String value.
                Value::from_raw(unsafe {
                    sys::mrb_integer_to_str(mrb.as_ptr(), self.0, base as sys::mrb_int)
                })
            })
            // SAFETY: a successful `mrb_integer_to_str` returns a
            // String-tagged value, so the unchecked wrap accepts it.
            .map(|v| unsafe { RString::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, base);
            crate::not_linked()
        }
    }

    /// Convert this Float value to the Integer value it truncates toward
    /// zero, the way Ruby's `Float#to_i` / `Float#to_int` core does — `3.9`
    /// to `3`, `-3.9` to `-3`. The result stays an mruby `Value`, an Integer
    /// in the VM's value domain, not a Rust scalar. `mrb_float_to_integer`
    /// guards its own receiver on the Float tag, raising `TypeError` for any
    /// other tag, and raises `RangeError` for an infinite or NaN float, which
    /// has no integer; both raises run under `Mrb::protect`, so either
    /// surfaces as `Err` rather than long-jumping. magnus's `Float` exposes no
    /// such conversion, so this anchors on mruby's own `mrb_float_to_integer`.
    #[inline]
    pub fn float_to_int(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_float_to_integer` raises
                // `TypeError` on a non-Float receiver and `RangeError` on an
                // infinite or NaN float — both caught by `protect` into `Err`
                // — and otherwise returns an Integer value.
                Value(unsafe { sys::mrb_float_to_integer(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Add `other` to `self`, Ruby's `+` on `Integer` and `Float` — `2 + 3`
    /// to `5`, `2 + 3.5` to `5.5`. The result stays an mruby `Value`: an
    /// Integer when both operands are integers and the result fits the
    /// configured integer width, a Float when either operand is a float, the
    /// mixed case widening the integer operand. `mrb_num_add` dispatches its
    /// receiver on the numeric tag, so a non-numeric operand raises `TypeError`
    /// and an integer result past the configured width raises `RangeError`;
    /// both run under `Mrb::protect`, surfacing as `Err` rather than
    /// long-jumping. magnus's `coerce_bin` routes through the full Ruby
    /// coercion protocol, which mruby has no counterpart to, so this anchors on
    /// mruby's own `mrb_num_add` (the obsolete macro `mrb_num_plus` aliases it).
    #[inline]
    pub fn add(self, mrb: &Mrb, other: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self` and
                // `other` originate from the same VM. `mrb_num_add` raises
                // `TypeError` on a non-numeric operand and `RangeError` on an
                // integer result past the configured width — both caught by
                // `protect` into `Err`.
                Value(unsafe { sys::mrb_num_add(mrb.as_ptr(), self.0, other.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// Subtract `other` from `self`, Ruby's `-` on `Integer` and `Float`. The
    /// result type and raises mirror `Value::add`: an Integer when both
    /// operands are integers and the result fits the configured width, a Float
    /// when either is a float; a non-numeric operand raises `TypeError` and an
    /// integer result past the configured width raises `RangeError`, both
    /// caught by `Mrb::protect`. Anchors on mruby's own `mrb_num_sub` (the
    /// obsolete macro `mrb_num_minus` aliases it).
    #[inline]
    pub fn sub(self, mrb: &Mrb, other: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `Value::add`. `mrb_num_sub` raises `TypeError` on
                // a non-numeric operand and `RangeError` on an integer result
                // past the configured width — both caught by `protect`.
                Value(unsafe { sys::mrb_num_sub(mrb.as_ptr(), self.0, other.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// Multiply `self` by `other`, Ruby's `*` on `Integer` and `Float`. The
    /// result type and raises mirror `Value::add`: an Integer when both
    /// operands are integers and the result fits the configured width, a Float
    /// when either is a float; a non-numeric operand raises `TypeError` and an
    /// integer result past the configured width raises `RangeError`, both
    /// caught by `Mrb::protect`. Anchors on mruby's own `mrb_num_mul`.
    #[inline]
    pub fn mul(self, mrb: &Mrb, other: Value) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `Value::add`. `mrb_num_mul` raises `TypeError` on
                // a non-numeric operand and `RangeError` on an integer result
                // past the configured width — both caught by `protect`.
                Value(unsafe { sys::mrb_num_mul(mrb.as_ptr(), self.0, other.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// Coerce `self` to a string value — `self` unchanged when it is
    /// already a string, otherwise the result of its `to_s`. Runs under
    /// `Mrb::protect`: `Ok` with the string value, or `Err` when `to_s`
    /// does not return a string. Mirrors mruby's `mrb_obj_as_string`.
    #[inline]
    pub fn obj_as_string(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_obj_as_string` may run
                // `to_s` and raise — caught by `protect` into `Err`.
                Value(unsafe { sys::mrb_obj_as_string(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` to a typed `RString` handle by its String tag,
    /// surfacing a non-String as an `Err` rather than rejecting it to
    /// `None`: `Ok` with the handle when `self` is String-tagged, `Err`
    /// carrying a `TypeError` for any other tag. It runs no user Ruby —
    /// it dispatches no `to_str` — so it is the raising counterpart to
    /// the `RString::from_value` downcast, not the dispatching `to_s`
    /// coercion `Value::obj_as_string` performs. The `TypeError` it would
    /// long-jump is caught by `Mrb::protect` into the returned `Err`.
    /// Suits a handler that requires a String argument and rejects
    /// anything else; reach for the `FromValue` downcast instead when a
    /// non-String should read as absent. Mirrors mruby's
    /// `mrb_ensure_string_type`.
    #[inline]
    pub fn ensure_string(self, mrb: &Mrb) -> Result<crate::RString, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ensure_string_type`
                // raises `TypeError` on a non-String tag — caught by
                // `protect` into `Err` — and otherwise returns `self`
                // unchanged.
                Value(unsafe { sys::mrb_ensure_string_type(mrb.as_ptr(), self.0) })
            })
            // SAFETY: an `Ok` result passed `mrb_string_p` inside
            // `mrb_ensure_string_type`, so it carries the String tag the
            // unchecked wrap requires.
            .map(|v| unsafe { RString::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` to a typed `Array` handle by its Array tag,
    /// surfacing a non-Array as an `Err` rather than rejecting it to
    /// `None`: `Ok` with the handle when `self` is Array-tagged, `Err`
    /// carrying a `TypeError` for any other tag. It runs no user Ruby —
    /// it dispatches no `to_ary` — so it is the raising counterpart to
    /// the `Array::from_value` downcast. The `TypeError` it would
    /// long-jump is caught by `Mrb::protect` into the returned `Err`.
    /// Suits a handler that requires an Array argument and rejects
    /// anything else; reach for the `FromValue` downcast instead when a
    /// non-Array should read as absent. Mirrors mruby's
    /// `mrb_ensure_array_type`.
    #[inline]
    pub fn ensure_array(self, mrb: &Mrb) -> Result<crate::Array, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ensure_array_type`
                // raises `TypeError` on a non-Array tag — caught by
                // `protect` into `Err` — and otherwise returns `self`
                // unchanged.
                Value(unsafe { sys::mrb_ensure_array_type(mrb.as_ptr(), self.0) })
            })
            // SAFETY: an `Ok` result passed `mrb_array_p` inside
            // `mrb_ensure_array_type`, so it carries the Array tag the
            // unchecked wrap requires.
            .map(|v| unsafe { crate::Array::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Spread `self` into a new typed `Array`, Ruby's `*` splat coercion:
    /// an array yields a copy of itself; a non-array that responds to
    /// `to_a` runs it, taking the result when it is an array and wrapping
    /// `self` in a one-element array when `to_a` returns `nil`; a value
    /// that answers no `to_a` wraps in a one-element array. It dispatches
    /// `to_a` and always yields an array, so it is the dispatching
    /// counterpart to `ensure_array`, which coerces by the Array tag alone
    /// and takes only an already-array value. A `TypeError` mruby raises
    /// when `to_a` returns a non-array non-`nil` value, or a raise from
    /// `to_a` itself, is caught by `Mrb::protect` into the returned `Err`.
    /// Mirrors mruby's `mrb_ary_splat`.
    #[inline]
    pub fn to_ary(self, mrb: &Mrb) -> Result<crate::Array, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ary_splat` dispatches
                // `to_a` for a non-array — a raise inside it, or a non-array
                // non-`nil` return, long-jumps a `TypeError` caught by
                // `protect` into `Err` — and otherwise returns an array.
                Value(unsafe { sys::mrb_ary_splat(mrb.as_ptr(), self.0) })
            })
            // SAFETY: `mrb_ary_splat` always returns an Array-tagged value
            // on the `Ok` path, the tag the unchecked wrap requires.
            .map(|v| unsafe { crate::Array::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` to a typed `Hash` handle by its Hash tag,
    /// surfacing a non-Hash as an `Err` rather than rejecting it to
    /// `None`: `Ok` with the handle when `self` is Hash-tagged, `Err`
    /// carrying a `TypeError` for any other tag. It runs no user Ruby —
    /// it dispatches no `to_hash` — so it is the raising counterpart to
    /// the `Hash::from_value` downcast. The `TypeError` it would
    /// long-jump is caught by `Mrb::protect` into the returned `Err`.
    /// Suits a handler that requires a Hash argument and rejects
    /// anything else; reach for the `FromValue` downcast instead when a
    /// non-Hash should read as absent. Mirrors mruby's
    /// `mrb_ensure_hash_type`.
    #[inline]
    pub fn ensure_hash(self, mrb: &Mrb) -> Result<crate::Hash, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ensure_hash_type`
                // raises `TypeError` on a non-Hash tag — caught by
                // `protect` into `Err` — and otherwise returns `self`
                // unchanged.
                Value(unsafe { sys::mrb_ensure_hash_type(mrb.as_ptr(), self.0) })
            })
            // SAFETY: an `Ok` result passed `mrb_hash_p` inside
            // `mrb_ensure_hash_type`, so it carries the Hash tag the
            // unchecked wrap requires.
            .map(|v| unsafe { crate::Hash::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` by numeric type to an Integer `Value`, staying in
    /// mruby's value domain rather than reading out a Rust scalar: an
    /// Integer returns unchanged, a Float truncates toward zero, and the
    /// result narrows to one that fits the configured integer width. It
    /// coerces between the numeric types, unlike the exact-tag
    /// `i32::from_value` downcast, and the `Value::as_int` sibling reads the
    /// same coercion out as a Rust `mrb_int`. It runs no user Ruby — it
    /// dispatches no `to_int` — so the `TypeError` mruby raises for a
    /// non-numeric value, or the `RangeError` it raises for an infinite or
    /// NaN Float, is caught by `Mrb::protect` into the returned `Err`.
    /// Mirrors mruby's `mrb_ensure_int_type` (over `mrb_ensure_integer_type`,
    /// which the width narrowing wraps).
    #[inline]
    pub fn ensure_int(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ensure_int_type` raises
                // `TypeError` on a non-numeric value and `RangeError` on an
                // infinite or NaN Float — both caught by `protect` into
                // `Err` — and otherwise returns an Integer value.
                Value(unsafe { sys::mrb_ensure_int_type(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` by numeric type to a Float `Value`, staying in mruby's
    /// value domain rather than reading out a Rust scalar: a Float returns
    /// unchanged and an Integer widens. It coerces between the numeric types,
    /// unlike the exact-tag `f64::from_value` downcast, and the
    /// `Value::as_float` sibling reads the same coercion out as a Rust
    /// `mrb_float`. It runs no user Ruby — it dispatches no `to_f` — so the
    /// `TypeError` mruby raises for a non-numeric value is caught by
    /// `Mrb::protect` into the returned `Err`. Mirrors mruby's
    /// `mrb_ensure_float_type`.
    #[inline]
    pub fn ensure_float(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_ensure_float_type` raises
                // `TypeError` on a non-numeric value — caught by `protect`
                // into `Err` — and otherwise returns a Float value.
                Value(unsafe { sys::mrb_ensure_float_type(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce `self` into a typed `Symbol`: a Symbol value yields its own
    /// id, a String value interns its contents, and any other value
    /// surfaces an `Err`. It runs no user Ruby — it dispatches no
    /// `to_sym` — so the `TypeError` mruby raises for a value that is
    /// neither a symbol nor a string is caught by `Mrb::protect` into the
    /// returned `Err`. Unlike `Symbol::new`, which interns Rust bytes,
    /// this coerces an existing mruby value. Mirrors mruby's
    /// `mrb_obj_to_sym`.
    #[inline]
    pub fn to_sym(self, mrb: &Mrb) -> Result<crate::Symbol, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_obj_to_sym` raises
                // `TypeError` for a value that is neither a symbol nor a
                // string — caught by `protect` into `Err` — and otherwise
                // returns the interned id.
                let sym = unsafe { sys::mrb_obj_to_sym(mrb.as_ptr(), self.0) };
                crate::Symbol::from_sym(sym).as_value()
            })
            // SAFETY: an `Ok` result came from `Symbol::from_sym`, so it
            // carries the Symbol tag the unchecked wrap requires.
            .map(|v| unsafe { crate::Symbol::from_value_unchecked(v) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `obj.dup` — a shallow copy of `self`: its instance variables are
    /// copied (not the objects they reference), the copy is unfrozen and
    /// carries no singleton class, and the class's `initialize_copy`
    /// runs on it. An immediate returns itself. Runs under `Mrb::protect`:
    /// `Ok` with the copy, or `Err` when `initialize_copy` raises.
    /// Mirrors mruby's `mrb_obj_dup`.
    #[inline]
    pub fn obj_dup(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_obj_dup` runs
                // `initialize_copy` and may raise — caught by `protect`.
                Value(unsafe { sys::mrb_obj_dup(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `obj.clone` — like `dup` but also copies the singleton class and
    /// the frozen state, the deeper of the two duplications; the class's
    /// `initialize_copy` runs on the copy. An immediate returns itself.
    /// Runs under `Mrb::protect`: `Ok` with the copy, or `Err` when
    /// `initialize_copy` raises. Mirrors mruby's `mrb_obj_clone`.
    #[inline]
    pub fn obj_clone(self, mrb: &Mrb) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_obj_clone` runs
                // `initialize_copy` and may raise — caught by `protect`.
                Value(unsafe { sys::mrb_obj_clone(mrb.as_ptr(), self.0) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_obj_classname(mrb, self)` — the Ruby class name of `self`
    /// as an owned `String`, or `""` when mruby returns NULL. mruby
    /// builds the name into a GC-managed temporary, so the bytes are
    /// copied out at once rather than borrowed.
    #[inline]
    pub fn classname(self, mrb: &Mrb) -> String {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the borrow; `self` originates
            // from the same VM by the single-VM contract.
            let ptr = unsafe { sys::mrb_obj_classname(mrb.as_ptr(), self.0) };
            if ptr.is_null() {
                return String::new();
            }
            // SAFETY: `ptr` is a valid C string for the duration of this
            // call; copy its bytes before the temporary it points into
            // can be collected.
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or("")
                .to_owned()
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Coerce to a Rust `String` by calling `Object#to_s` and copying
    /// the bytes by length. `String#to_s` is idempotent on mruby
    /// Strings, so the redundant call is cheap and keeps a single
    /// conversion entry point.
    ///
    /// Bytes are read through `RString::as_bytes` (RSTRING_PTR / RSTRING_LEN),
    /// not as a C string: an embedded NUL is a valid UTF-8 codepoint
    /// and must survive, yet `mrb_str_to_cstr` truncates at and raises
    /// on a NUL — and on the outcome-encode path (a `#eval` / `#run`
    /// result, a Panic message) that raise has no protect frame and
    /// aborts the guest. Bytes that are not valid UTF-8 collapse to an
    /// empty `String`.
    ///
    /// ## Exception handling
    ///
    /// If `.to_s` raises (a user object overrides it with `raise`) or
    /// returns a non-String, the failure is **swallowed**: an empty
    /// `String` is returned. The dispatch runs through `funcall`, whose
    /// `protect` frame catches the raise into `Err` and leaves no pending
    /// `mrb->exc` to corrupt subsequent mruby calls in the same C bridge.
    #[inline]
    pub fn to_string(self, mrb: &Mrb) -> String {
        #[cfg(mruby_linked)]
        {
            let Ok(s_val) = self.funcall(mrb, c"to_s", &[]) else {
                return String::new();
            };
            s_val.string_lossy(mrb)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Read a String-tagged value into an owned UTF-8 `String`,
    /// collapsing a non-String tag or non-UTF-8 bytes to an empty
    /// string — the shared render tail of `to_string` and `inspect`.
    /// The String tag, not the classname, decides: a String subclass
    /// instance reads its bytes the same way a plain String does, the
    /// rule the `FromValue` downcasts follow.
    #[cfg(mruby_linked)]
    #[inline]
    fn string_lossy(self, mrb: &Mrb) -> String {
        let Some(s) = RString::from_value(self) else {
            return String::new();
        };
        // SAFETY: `from_value` confirmed the String tag; the bytes are
        // copied before any further mruby call.
        let bytes = unsafe { s.as_bytes(mrb) };
        core::str::from_utf8(bytes).unwrap_or("").to_string()
    }

    /// `mrb_inspect(mrb, self)` — the value's debug string, Ruby's
    /// `inspect`, copied out as an owned Rust `String`. The inspect
    /// counterpart to `to_string`'s `to_s` render path, and infallible
    /// the same way.
    ///
    /// ## Exception handling
    ///
    /// `mrb_inspect` dispatches the receiver's `inspect` (falling back to
    /// `to_s` when that does not return a String), so a user-defined
    /// `inspect` that raises is **swallowed**: an empty `String` is
    /// returned. The dispatch runs under `Mrb::protect`, whose frame
    /// catches the raise into `Err` and leaves no pending `mrb->exc` to
    /// corrupt later mruby calls in the same C bridge. Bytes that are not
    /// valid UTF-8 likewise collapse to an empty `String`.
    #[inline]
    pub fn inspect(self, mrb: &Mrb) -> String {
        #[cfg(mruby_linked)]
        {
            let Ok(s_val) = mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_inspect` dispatches
                // `inspect` and may raise — caught by `protect` into `Err`.
                Value::from_raw(unsafe { sys::mrb_inspect(mrb.as_ptr(), self.0) })
            }) else {
                return String::new();
            };
            // `mrb_inspect` returns a String on success; read it by tag the
            // same way `to_string` does.
            s_val.string_lossy(mrb)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_any_to_s(mrb, self)` — the value's default `to_s` render as a
    /// new `RString`: `#<ClassName>` for an immediate, `#<ClassName:0x...>`
    /// for a heap object. Built from the class name without dispatching the
    /// value's own `to_s`, so it is the render `obj_as_string` falls back to
    /// and runs no user Ruby — total, returning the string directly.
    #[inline]
    pub fn any_to_s(self, mrb: &Mrb) -> crate::RString {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` originates from the same VM.
            // `mrb_any_to_s` reads the class name and object id only, so it
            // returns a String-tagged value without dispatching user Ruby —
            // the unchecked wrap accepts it.
            let v = Value::from_raw(unsafe { sys::mrb_any_to_s(mrb.as_ptr(), self.0) });
            unsafe { RString::from_value_unchecked(v) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Recover the `*mut RClass` pointer from a class-tagged
    /// `Value`, via the `mrb_class_ptr_func` static-inline wrapper in
    /// `wrapper.h` — the `mrb_class_ptr(v)` macro expands inside the
    /// C compiler, which sees the same boxing config the linked
    /// archive was built with.
    ///
    /// # Safety
    ///
    /// `self` must be a class-tagged `Value`.
    #[inline]
    pub unsafe fn as_class_ptr(self) -> *mut sys::RClass {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller.
            unsafe { sys::mrb_class_ptr_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Invoke `self.<method>(args...)`, naming the method by a
    /// symbol-or-name key (`IntoSym`): a string name interns through
    /// `Mrb::intern_cstr`, an already-interned `Symbol` is reused without
    /// re-interning. The method runs arbitrary Ruby, so the call runs
    /// under `Mrb::protect`: a normal return is the `Ok` value, any raise
    /// is `Err` rather than a long-jump across FFI. Use
    /// `Value::funcall_argv` when the caller already holds an interned
    /// `sys::mrb_sym` (e.g. a dispatch site that cached the sym across a
    /// `respond_to?` gate). Mirrors magnus's `funcall`.
    #[inline]
    pub fn funcall<K: crate::IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        args: &[Value],
    ) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            self.funcall_argv(mrb, sym, args)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, args);
            crate::not_linked()
        }
    }

    /// `mrb_funcall_argv(mrb, self, sym, argc, argv)` — invoke the method
    /// already interned as `sym`, under `Mrb::protect`. Counterpart to
    /// `Value::funcall` for sites that pre-intern (typically because the
    /// same symbol is queried via `respond_to?` first). The dispatched
    /// method runs arbitrary Ruby and may raise, which `protect` catches
    /// into `Err` rather than long-jumping across FFI.
    ///
    /// `args` is `&[Value]`; `Value` is `#[repr(transparent)]` over
    /// `mrb_value`, so the slice layout matches mruby's `mrb_value`
    /// argv exactly — the pointer cast on the way through is a no-op
    /// at codegen level.
    #[inline]
    pub fn funcall_argv(
        self,
        mrb: &Mrb,
        sym: sys::mrb_sym,
        args: &[Value],
    ) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                let argv = args.as_ptr() as *const sys::mrb_value;
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // and every `args` entry originate from the same VM by the
                // single-VM contract; `sym` was interned against the same
                // VM (caller contract). `mrb_funcall_argv` dispatches
                // arbitrary Ruby and may raise — caught by `protect`.
                Value(unsafe {
                    sys::mrb_funcall_argv(
                        mrb.as_ptr(),
                        self.0,
                        sym,
                        args.len() as sys::mrb_int,
                        argv,
                    )
                })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym, args);
            crate::not_linked()
        }
    }

    /// `mrb_funcall_with_block(mrb, self, sym, argc, argv, block)` —
    /// invoke the method named by `name` with `args`, handing it `block`
    /// for the method to yield to, under `Mrb::protect`. The block-passing
    /// counterpart to `Value::funcall`: a method wanting no block uses
    /// `funcall`/`funcall_argv` rather than this with a nil block. The
    /// dispatched method runs arbitrary Ruby and may raise, which `protect`
    /// catches into `Err` rather than long-jumping across FFI. Mirrors
    /// magnus's `funcall_with_block`.
    #[inline]
    pub fn funcall_with_block<K: crate::IntoSym>(
        self,
        mrb: &Mrb,
        name: K,
        args: &[Value],
        block: crate::Proc,
    ) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = name.into_sym(mrb);
            let block_raw = block.as_raw();
            mrb.protect(|mrb| {
                // `Value` is `#[repr(transparent)]` over `mrb_value`, so the
                // slice layout matches mruby's argv exactly — the cast is a
                // no-op at codegen level.
                let argv = args.as_ptr() as *const sys::mrb_value;
                // SAFETY: `mrb` is alive inside the protect frame; `self`,
                // every `args` entry, and `block` originate from the same VM
                // by the single-VM contract; `sym` was interned against the
                // same VM. `mrb_funcall_with_block` dispatches arbitrary Ruby
                // and may raise — caught by `protect`.
                Value(unsafe {
                    sys::mrb_funcall_with_block(
                        mrb.as_ptr(),
                        self.0,
                        sym,
                        args.len() as sys::mrb_int,
                        argv,
                        block_raw,
                    )
                })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, args, block);
            crate::not_linked()
        }
    }

    /// TRUE when `self` is `nil`. Pure tag predicate via mruby's
    /// `mrb_nil_p(v)`, reached through bindgen's static-fn trampoline
    /// — the `wrapper.h` shim wraps the macro so the C compiler reads
    /// the boxing-config layout libmruby.a was built with.
    #[inline]
    pub fn is_nil(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_nil_p is a pure predicate over the value tag and
            // does not touch `mrb_state`.
            unsafe { sys::mrb_nil_p_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Ruby truthiness: TRUE for every value except `nil` and `false`.
    /// This is the `if` test, not a type check — routes through mruby's
    /// `mrb_test` shim so the boxing-config layout matches the linked
    /// archive, like `Value::is_nil`. Pair with `FromValue for bool`,
    /// which reads a value through this rule.
    #[inline]
    pub fn to_bool(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_test is a pure predicate over the value tag and
            // does not touch `mrb_state`.
            unsafe { sys::mrb_test_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` is exactly Ruby `true`. See `Value::is_nil` for
    /// the boxing-config routing.
    #[inline]
    pub fn is_true(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_true_p is a pure predicate over the value tag and
            // does not touch `mrb_state`.
            unsafe { sys::mrb_true_p_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` is exactly Ruby `false` — `nil` is excluded.
    /// `nil` and `false` share the `MRB_TT_FALSE` tag under some boxing
    /// modes, so this must route through mruby's `mrb_false_p` shim
    /// rather than a tag test, which would misread `nil`.
    #[inline]
    pub fn is_false(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_false_p is a pure predicate over the value tag and
            // does not touch `mrb_state`.
            unsafe { sys::mrb_false_p_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_INTEGER`. Pure tag predicate
    /// via mruby's `mrb_type` (`MRB_INLINE`), reached through
    /// bindgen's static-fn trampoline. Pair with
    /// `Value::unbox_integer` for the direct-unbox path.
    #[inline]
    pub fn is_integer(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_type is a pure predicate over the value tag and
            // does not touch `mrb_state`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_INTEGER }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_FLOAT`. See `Value::is_integer`.
    /// Pair with `Value::unbox_float`.
    #[inline]
    pub fn is_float(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_FLOAT }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_ARRAY`. See `Value::is_integer`.
    /// Pair with `Array::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_array(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_ARRAY }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_HASH`. See `Value::is_integer`.
    /// Pair with `Hash::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_hash(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_HASH }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_CLASS` — the class tag only;
    /// modules (`MRB_TT_MODULE`) and singleton classes
    /// (`MRB_TT_SCLASS`) are excluded per SPEC's downcast rule. See
    /// `Value::is_integer`. Pair with `Value::as_class_ptr` for the
    /// direct-unbox path.
    #[inline]
    pub fn is_class(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_CLASS }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_MODULE` — the module tag only;
    /// classes (`MRB_TT_CLASS`) are excluded, the complement of
    /// `Value::is_class`. See `Value::is_integer`. No typed handle binds
    /// this tag yet, so the predicate stands alone.
    #[inline]
    pub fn is_module(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_MODULE }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_PROC`. See `Value::is_integer`.
    /// Pair with `Proc::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_proc(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_PROC }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_CDATA` — a Rust value wrapped
    /// through the data-carrier seam. See `Value::is_integer`. Pair
    /// with `Value::data_get` for the type-checked extraction path.
    #[inline]
    pub fn is_data(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_CDATA }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_STRING`. See `Value::is_integer`.
    /// Pair with `RString::as_bytes` for the byte-borrow path.
    #[inline]
    pub fn is_string(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_STRING }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_SYMBOL`. See `Value::is_integer`.
    /// Pair with `Symbol::from_value` for the checked downcast path.
    #[inline]
    pub fn is_symbol(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_SYMBOL }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_RANGE`. See `Value::is_integer`.
    /// No typed handle binds this tag yet, so the predicate stands alone.
    #[inline]
    pub fn is_range(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_RANGE }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// TRUE when `self` carries `MRB_TT_EXCEPTION` — the exception-object
    /// tag, the type every `raise`d value carries; an arbitrary class that
    /// merely descends from `Exception` is not yet an instance and reads
    /// FALSE. See `Value::is_integer`. No typed handle binds this tag yet,
    /// so the predicate stands alone.
    #[inline]
    pub fn is_exception(self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_integer`.
            unsafe { sys::mrb_type(self.0) == sys::MRB_TT_EXCEPTION }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// View `self` as a typed `Break` when it carries mruby's break
    /// tag (`MRB_TT_BREAK`), or `None` for any other tag. A break
    /// surfaces as the value inside the `Err` of a protected
    /// `Proc::call` when the block exits via a non-local `break` or
    /// `return`; classifying that exit is the caller's policy.
    #[inline]
    pub fn as_break(self) -> Option<Break> {
        #[cfg(mruby_linked)]
        {
            // SAFETY: mrb_break_p_func is a pure predicate over the
            // value tag and does not touch mrb_state. The tag check
            // establishes the `Break` newtype's invariant.
            unsafe { sys::mrb_break_p_func(self.0) }.then_some(Break(self))
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Direct `mrb_integer(v)` unbox via mruby's own
    /// `mrb_integer_func` helper (a `MRB_INLINE` reached through
    /// bindgen's static-fn trampoline).
    ///
    /// # Safety
    ///
    /// Caller must have confirmed Integer-tagging via
    /// `Value::is_integer`; calling on a non-Integer is undefined
    /// behaviour per mruby's macro contract.
    #[inline]
    pub unsafe fn unbox_integer(self) -> sys::mrb_int {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller.
            unsafe { sys::mrb_integer_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Direct `mrb_float(v)` unbox via the `mrb_float_func`
    /// static-inline wrapper in `wrapper.h`. The `mrb_float(o)` macro
    /// expands differently per boxing mode (inline-rotated word,
    /// RFloat heap read, NaN payload); expanding it inside the C
    /// compiler keeps the unbox correct for whatever config the
    /// linked archive was built with.
    ///
    /// # Safety
    ///
    /// As `Value::unbox_integer`: caller has confirmed Float-tagging.
    #[inline]
    pub unsafe fn unbox_float(self) -> sys::mrb_float {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller.
            unsafe { sys::mrb_float_func(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_ary_entry(self, idx)` — read the element at `idx` from
    /// `self` (which must be an Array `Value`). No bounds checking;
    /// caller must keep `idx` within `0..self.length`.
    ///
    /// # Safety
    ///
    /// `self` must be an Array-tagged `Value`. Out-of-range `idx`
    /// returns `mrb_nil_value` rather than reading past the buffer;
    /// passing a non-Array yields an undefined `Value`.
    #[inline]
    pub unsafe fn ary_entry(self, idx: sys::mrb_int) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller.
            Value(unsafe { sys::mrb_ary_entry(self.0, idx) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = idx;
            crate::not_linked()
        }
    }

    // ----------------------------------------------------------------
    // Instance variable / constant / class variable accessors. The
    // mruby C API spells these as `mrb_iv_set` / `mrb_iv_get` /
    // `mrb_const_set` / `mrb_const_get` / `mrb_cv_set` / `mrb_cv_get` /
    // `mrb_const_defined` / `mrb_respond_to`; the inherent methods
    // carry the same names so the call shape mirrors the C-side
    // documentation one-to-one. The reads (`iv_get`, `const_defined`,
    // `respond_to`) dispatch nothing and hand back a bare value; the
    // assigning and fetching operations (`iv_set`, `const_set`,
    // `const_get`, `cv_set`, `cv_get`) can raise, so they route through
    // `protect` and return a `Result`.
    // ----------------------------------------------------------------

    /// `mrb_iv_set(mrb, self, sym, val)` — assign instance variable
    /// `sym` on `self` to `val`. Surfaces an `Err` when `self` is
    /// frozen or cannot hold instance variables.
    #[inline]
    pub fn iv_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `val` originate from the same VM.
                // `mrb_iv_set` raises `FrozenError` on a frozen
                // receiver and `ArgumentError` on one that cannot hold
                // instance variables — both caught by `protect`.
                unsafe { sys::mrb_iv_set(mrb.as_ptr(), self.0, sym, val.0) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym, val);
            crate::not_linked()
        }
    }

    /// `mrb_iv_get(mrb, self, sym)` — return instance variable `sym`
    /// from `self`, or `nil` when unset.
    #[inline]
    pub fn iv_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            Value(unsafe { sys::mrb_iv_get(mrb.as_ptr(), self.0, sym) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_iv_defined(mrb, self, sym)` — TRUE when instance variable
    /// `sym` is set on `self`. A receiver that cannot hold instance
    /// variables reads as FALSE rather than raising. The value-level
    /// analogue of the raw-`RObject*` `mrb_obj_iv_defined`, which stays
    /// in `sys`.
    #[inline]
    pub fn iv_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            unsafe { sys::mrb_iv_defined(mrb.as_ptr(), self.0, sym) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_iv_remove(mrb, self, sym)` — remove instance variable `sym`
    /// from `self`, returning `Some` of its former value. Yields `None`
    /// when the variable is absent or `self` cannot hold instance
    /// variables, distinguishing either case from a variable removed
    /// while holding `nil`. Surfaces an `Err` only when a frozen `self`
    /// can hold instance variables.
    #[inline]
    pub fn iv_remove(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Option<Value>, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` originates from the same VM. `mrb_iv_remove`
                // raises `FrozenError` on a frozen instance-variable
                // holder — caught by `protect`.
                Value(unsafe { sys::mrb_iv_remove(mrb.as_ptr(), self.0, sym) })
            })
            .map(|removed| {
                // SAFETY: a total tag read on the protected result.
                if unsafe { sys::mrb_undef_p_func(removed.0) } {
                    None
                } else {
                    Some(removed)
                }
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_iv_foreach(mrb, self, …)` — visit each set instance variable
    /// on `self` in iv-table order, handing its name as a typed `Symbol`
    /// and its value to `body`. Returning `ForEach::Stop` ends the walk
    /// before the remaining variables; `ForEach::Continue` proceeds. A
    /// receiver that holds no instance variables — an immediate, or one
    /// that never had any — is visited zero times. The walk dispatches no
    /// Ruby and so never raises; magnus binds no ivar foreach, so this
    /// anchors on mruby's own `mrb_iv_foreach`.
    ///
    /// Mutating the receiver's instance variables from within `body` is
    /// unsupported: the C foreach indexes the iv table it captured at
    /// entry, which a mutation can reallocate.
    ///
    /// A panic in `body` is caught at the FFI boundary, stops the walk,
    /// and resurfaces here once `mrb_iv_foreach` returns — it never
    /// unwinds into mruby's C frames.
    #[inline]
    pub fn each_iv<F>(self, mrb: &Mrb, body: F)
    where
        F: FnMut(crate::Symbol, Value) -> crate::ForEach,
    {
        #[cfg(mruby_linked)]
        {
            // Park the closure beside a panic slot in a stack local. The
            // trampoline borrows it per variable; on a panic it stashes
            // the unwind payload here and reports `Stop`, so the C walk
            // ends without a panic crossing its frames. The payload
            // resumes below once control is back on the Rust side.
            struct Walk<F> {
                body: F,
                panic: Option<Box<dyn std::any::Any + Send>>,
            }

            unsafe extern "C" fn trampoline<F>(
                _mrb: *mut sys::mrb_state,
                name: sys::mrb_sym,
                val: sys::mrb_value,
                data: *mut core::ffi::c_void,
            ) -> core::ffi::c_int
            where
                F: FnMut(crate::Symbol, Value) -> crate::ForEach,
            {
                // SAFETY: `data` is the `&mut Walk<F>` handed to
                // `mrb_iv_foreach` below; the foreach call borrows it for
                // the duration of the walk on this same thread.
                let walk: &mut Walk<F> = unsafe { &mut *(data as *mut Walk<F>) };
                let name = crate::Symbol::from_sym(name);
                let val = Value::from_raw(val);
                // Catch here so a `body` panic stops the walk instead of
                // unwinding through `mrb_iv_foreach`'s C frame.
                // AssertUnwindSafe matches the crate's other panic
                // boundaries: the parked payload is the only state that
                // survives the catch. A non-zero return stops the C walk,
                // so the trampoline is not re-entered after `Stop` or a
                // parked panic.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (walk.body)(name, val)
                })) {
                    Ok(crate::ForEach::Continue) => 0,
                    Ok(crate::ForEach::Stop) => 1,
                    Err(payload) => {
                        walk.panic = Some(payload);
                        1
                    }
                }
            }

            let mut walk = Walk { body, panic: None };
            // SAFETY: `mrb` is alive; `self` originates from the same VM.
            // `mrb_iv_foreach` guards a receiver that cannot hold instance
            // variables and returns without calling back. `trampoline::<F>`
            // upholds the `mrb_iv_foreach_func` ABI; `data` points to
            // `walk` on this frame, which outlives the call. bindgen wraps
            // the function-typedef parameter in `Option`, so the
            // trampoline is passed via `Some`.
            unsafe {
                sys::mrb_iv_foreach(
                    mrb.as_ptr(),
                    self.0,
                    Some(trampoline::<F>),
                    &mut walk as *mut Walk<F> as *mut core::ffi::c_void,
                );
            }
            if let Some(payload) = walk.panic {
                std::panic::resume_unwind(payload);
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, body);
            crate::not_linked()
        }
    }

    /// `mrb_const_defined(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined on `self` (the module or class value), walking the
    /// ancestry.
    #[inline]
    pub fn const_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            unsafe { sys::mrb_const_defined(mrb.as_ptr(), self.0, sym) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_const_defined_at(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined directly on `self` alone, never one inherited from an
    /// ancestor; contrast `const_defined`, which walks the ancestry.
    #[inline]
    pub fn const_defined_at(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            unsafe { sys::mrb_const_defined_at(mrb.as_ptr(), self.0, sym) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_const_get(mrb, self, sym)` — fetch the constant value at
    /// `sym` from `self`. Surfaces an `Err` when `sym` resolves to no
    /// constant or its `const_missing` hook raises.
    #[inline]
    pub fn const_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` originates from the same VM. `mrb_const_get`
                // raises `NameError` for an undefined constant and runs
                // a `const_missing` hook that may raise — both caught
                // by `protect`.
                Value(unsafe { sys::mrb_const_get(mrb.as_ptr(), self.0, sym) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_const_set(mrb, self, sym, val)` — assign constant `sym` on
    /// `self` (the module or class value) to `val`. Surfaces an `Err`
    /// when `self` is not a class or module, when `self` is frozen, or
    /// when the `const_added` hook raises. The value-level write
    /// complementing `const_get`.
    #[inline]
    pub fn const_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `val` originate from the same VM.
                // `mrb_const_set` raises `TypeError` when `self` is not
                // a class or module, `FrozenError` when it is frozen,
                // and runs a `const_added` hook that may raise — all
                // caught by `protect`.
                unsafe { sys::mrb_const_set(mrb.as_ptr(), self.0, sym, val.0) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym, val);
            crate::not_linked()
        }
    }

    /// `mrb_const_remove(mrb, self, sym)` — remove constant `sym` from
    /// `self` (the module or class value), discarding its former value.
    /// An absent constant is a no-op; surfaces an `Err` when `self` is
    /// not a class or module, or when it is frozen.
    #[inline]
    pub fn const_remove(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` originates from the same VM. `mrb_const_remove`
                // raises `TypeError` when `self` is not a class or
                // module and `FrozenError` when it is frozen — both
                // caught by `protect`.
                unsafe { sys::mrb_const_remove(mrb.as_ptr(), self.0, sym) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_cv_get(mrb, self, sym)` — read class variable `sym` from
    /// `self` (the module or class value), walking the ancestry.
    /// Surfaces an `Err` when `sym` resolves to no class variable.
    #[inline]
    pub fn cv_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` originates from the same VM. `mrb_cv_get`
                // raises `NameError` for an undefined class variable —
                // caught by `protect`.
                Value(unsafe { sys::mrb_cv_get(mrb.as_ptr(), self.0, sym) })
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_cv_set(mrb, self, sym, val)` — assign class variable `sym`
    /// on `self` (the module or class value) to `val`. Surfaces an
    /// `Err` when `self` is frozen. The value-level write complementing
    /// `cv_get`; `mrb_mod_cv_set` (the raw-`RClass*` form) stays in
    /// `sys`.
    #[inline]
    pub fn cv_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame;
                // `self` and `val` originate from the same VM.
                // `mrb_cv_set` raises `FrozenError` on a frozen
                // receiver — caught by `protect`.
                unsafe { sys::mrb_cv_set(mrb.as_ptr(), self.0, sym, val.0) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym, val);
            crate::not_linked()
        }
    }

    /// `mrb_cv_defined(mrb, self, sym)` — TRUE when class variable `sym`
    /// is defined on `self` (the module or class value) or any ancestor.
    /// The value-level analogue of the raw-`RClass*` `mrb_mod_cv_defined`,
    /// which stays in `sys`.
    #[inline]
    pub fn cv_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            unsafe { sys::mrb_cv_defined(mrb.as_ptr(), self.0, sym) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, sym);
            crate::not_linked()
        }
    }

    /// `mrb_respond_to(mrb, self, mid)` — TRUE when `self` answers to
    /// the method named by `mid`.
    #[inline]
    pub fn respond_to(self, mrb: &Mrb, mid: sys::mrb_sym) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            unsafe { sys::mrb_respond_to(mrb.as_ptr(), self.0, mid) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, mid);
            crate::not_linked()
        }
    }

    /// `mrb_obj_class(mrb, self)` — the class `self` belongs to, Ruby's
    /// `Object#class`. Every value has a class, so this never fails.
    #[inline]
    pub fn class(self, mrb: &Mrb) -> RClass {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` shares the VM. `mrb_obj_class`
            // returns the receiver's class pointer, never null.
            RClass::from_raw(unsafe { sys::mrb_obj_class(mrb.as_ptr(), self.0) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_singleton_class(mrb, self)` — the value's own singleton class,
    /// Ruby's `singleton_class`: the per-instance eigenclass that holds
    /// methods defined on that one object, distinct from the regular class
    /// `Value::class` returns and shared with its peers. It is created on
    /// first read and stable across re-reads of the same object. `nil`,
    /// `true`, and `false` yield their predefined classes, which act as
    /// their singleton classes; every other immediate — an integer, a
    /// symbol, a float — has no singleton class, and the `TypeError` mruby
    /// raises is caught by `Mrb::protect` into the returned `Err`. The raw
    /// `RClass*` form (`mrb_singleton_class_ptr`), which hands back a
    /// possibly-null pointer and demands VM-internal reasoning, stays behind
    /// `beni::sys`. Mirrors magnus's `Object::singleton_class`.
    #[inline]
    pub fn singleton_class(self, mrb: &Mrb) -> Result<RClass, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_singleton_class` raises
                // `TypeError` for an immediate that has no singleton class —
                // caught by `protect` into `Err` — and otherwise returns a
                // class-tagged value.
                Value::from_raw(unsafe { sys::mrb_singleton_class(mrb.as_ptr(), self.0) })
            })
            // SAFETY: an `Ok` result is the class-tagged value
            // `mrb_singleton_class` returns, so the pointer recovery accepts it.
            .map(|v| RClass::from_raw(unsafe { v.as_class_ptr() }))
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_obj_is_kind_of(mrb, self, class)` — whether `self` is an
    /// instance of `class` or any of its subclasses, Ruby's `is_a?`. A
    /// pure ancestry walk that dispatches nothing, so it never raises.
    #[inline]
    pub fn is_kind_of(self, mrb: &Mrb, class: RClass) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` and `class` share the VM.
            // `mrb_obj_is_kind_of` only walks the class hierarchy.
            unsafe { sys::mrb_obj_is_kind_of(mrb.as_ptr(), self.0, class.as_raw()) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, class);
            crate::not_linked()
        }
    }

    /// `mrb_obj_is_instance_of(mrb, self, class)` — whether `self` is a
    /// direct instance of `class`, Ruby's `instance_of?`. A pure class
    /// compare that dispatches nothing, so it never raises.
    #[inline]
    pub fn is_instance_of(self, mrb: &Mrb, class: RClass) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `is_kind_of`; `mrb_obj_is_instance_of` only reads
            // the receiver's class.
            unsafe {
                sys::mrb_obj_is_instance_of(
                    mrb.as_ptr(),
                    self.0,
                    class.as_raw() as *const sys::RClass,
                )
            }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, class);
            crate::not_linked()
        }
    }

    /// `mrb_obj_freeze(mrb, self)` — freeze `self` in place and return
    /// it, Ruby's `Object#freeze`. Freezing is idempotent and never
    /// raises.
    #[inline]
    pub fn freeze(self, mrb: &Mrb) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` shares the VM. `mrb_obj_freeze`
            // sets the frozen flag and returns the receiver.
            Value::from_raw(unsafe { sys::mrb_obj_freeze(mrb.as_ptr(), self.0) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_check_frozen_value(mrb, self)` — a precondition guard that
    /// surfaces an `Err` when `self` is frozen, `Ok(())` otherwise. An
    /// immediate counts as frozen. Runs no user Ruby; the `FrozenError`
    /// it would long-jump is caught by `Mrb::protect` into the returned
    /// `Err`. The magnus-aligned way a handler rejects a write to a frozen
    /// receiver before attempting it — mruby's own mutating operations
    /// already perform this check internally, so this is the early-guard
    /// form, not a prerequisite for them.
    #[inline]
    pub fn check_frozen(self, mrb: &Mrb) -> Result<(), Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_check_frozen_value`
                // raises `FrozenError` on a frozen or immediate receiver —
                // caught by `protect`.
                unsafe { sys::mrb_check_frozen_value(mrb.as_ptr(), self.0) };
                Value::nil()
            })
            .map(|_| ())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_obj_equal(mrb, self, other)` — TRUE when `self` and `other`
    /// are the same object, Ruby's `equal?`. A pure identity compare:
    /// it dispatches nothing, so it never raises and yields a `bool`.
    #[inline]
    pub fn obj_equal(self, mrb: &Mrb, other: Value) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive; `self` and `other` share the VM by
            // the single-VM contract. `mrb_obj_equal` only inspects the
            // two values' identity.
            unsafe { sys::mrb_obj_equal(mrb.as_ptr(), self.0, other.0) }
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_obj_id(self)` — a unique integer identifier for `self`,
    /// Ruby's `object_id`. Reads the value's identity from the boxed
    /// word alone, so it takes no `Mrb`, dispatches nothing, and never
    /// raises.
    #[inline]
    pub fn object_id(self) -> sys::mrb_int {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb_obj_id` reads only `self`'s boxed word for its
            // identity and does not touch `mrb_state`.
            unsafe { sys::mrb_obj_id(self.0) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// `mrb_equal(mrb, self, other)` — Ruby `==` equality. May run a
    /// user-defined `==`, so it runs under the same protection as
    /// `Mrb::protect`: `Ok(bool)` for the comparison, or `Err` when the
    /// dispatched method raises.
    #[inline]
    pub fn equal(self, mrb: &Mrb, other: Value) -> Result<bool, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive; `self` and `other` share the
                // VM. `mrb_equal` may dispatch `==` and raise, which
                // `protect` catches into `Err`.
                let eq = unsafe { sys::mrb_equal(mrb.as_ptr(), self.0, other.0) };
                if eq {
                    Value::true_()
                } else {
                    Value::false_()
                }
            })
            .map(|v| v.to_bool())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_eql(mrb, self, other)` — Ruby `eql?`, the stricter equality
    /// `Hash` keys use. May run a user-defined `eql?`, so like `equal`
    /// it runs under protection: `Ok(bool)` or `Err` on a raise.
    #[inline]
    pub fn eql(self, mrb: &Mrb, other: Value) -> Result<bool, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `equal`; `mrb_eql` may dispatch `eql?` and
                // raise, caught by `protect`.
                let eq = unsafe { sys::mrb_eql(mrb.as_ptr(), self.0, other.0) };
                if eq {
                    Value::true_()
                } else {
                    Value::false_()
                }
            })
            .map(|v| v.to_bool())
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_cmp(mrb, self, other)` — Ruby's `<=>` three-way comparison.
    /// Dispatches a user-defined `<=>`, so it runs under `Mrb::protect`:
    /// `Ok(Some(ordering))` ranks the values by the sign of the result —
    /// negative, zero, or positive — following the `<=>` contract rather
    /// than assuming a -1 / 0 / 1 magnitude. `Ok(None)` yields nothing when
    /// the values are incomparable (Ruby `<=>` yielding `nil`), and `Err`
    /// when the dispatched comparison raises. Distinct from `equal` /
    /// `eql`, which test sameness rather than rank.
    #[inline]
    pub fn cmp(self, mrb: &Mrb, other: Value) -> Result<Option<core::cmp::Ordering>, Error> {
        #[cfg(mruby_linked)]
        {
            // `mrb_cmp` reserves -2 to flag two incomparable values.
            const INCOMPARABLE: sys::mrb_int = -2;
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // and `other` share the VM. `mrb_cmp` may dispatch `<=>`
                // and raise, caught by `protect`. It returns the sign of
                // `<=>` for numeric / String receivers and passes a custom
                // `<=>` result through unnormalized otherwise, reserving -2
                // as the incomparable sentinel; the result re-boxes
                // losslessly through `from_int`.
                let n = unsafe { sys::mrb_cmp(mrb.as_ptr(), self.0, other.0) };
                Value::from_int(mrb, n)
            })
            // SAFETY: the `Ok` value was boxed by `Value::from_int` just
            // above, so it carries an Integer tag the unbox accepts.
            .map(|v| match unsafe { v.unbox_integer() } {
                // -2 is the dedicated incomparable sentinel; every other
                // value ranks by its sign, since Ruby's `<=>` contract
                // only promises negative / zero / positive.
                INCOMPARABLE => None,
                0 => Some(core::cmp::Ordering::Equal),
                n if n < 0 => Some(core::cmp::Ordering::Less),
                _ => Some(core::cmp::Ordering::Greater),
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, other);
            crate::not_linked()
        }
    }

    /// `mrb_as_int(mrb, self)` — convert `self` to a Rust integer across
    /// the numeric types: an Integer reads directly and a Float truncates
    /// toward zero. A non-numeric value raises `TypeError` and a Float
    /// that is infinite or NaN raises `RangeError`, so the conversion runs
    /// under `Mrb::protect`: `Ok` with the number, or `Err`. The
    /// conversion runs no user Ruby — it dispatches no `to_int`. Distinct
    /// from `i32::from_value`, the exact-tag downcast that never converts
    /// across types and rejects a Float outright.
    ///
    /// The converted number round-trips through `Value::from_int` inside
    /// the protect frame and `unbox_integer` after — `mrb_int_value` is
    /// the boxing-agnostic constructor (heap bigint when the value
    /// exceeds the inline range) and `mrb_integer` reads either form
    /// back, so the round-trip is lossless across the full `mrb_int`
    /// range.
    #[inline]
    pub fn as_int(self, mrb: &Mrb) -> Result<sys::mrb_int, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: `mrb` is alive inside the protect frame; `self`
                // originates from the same VM. `mrb_as_int` raises
                // `TypeError` on a non-numeric value and `RangeError` on
                // an infinite / NaN float — both caught by `protect`. The
                // result re-boxes losslessly through `from_int`.
                let n = unsafe { sys::mrb_as_int_func(mrb.as_ptr(), self.0) };
                Value::from_int(mrb, n)
            })
            // SAFETY: the `Ok` value was boxed by `Value::from_int` just
            // above, so it carries an Integer tag the unbox accepts.
            .map(|v| unsafe { v.unbox_integer() })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// `mrb_as_float(mrb, self)` — convert `self` to a Rust float across
    /// the numeric types: a Float reads directly and an Integer widens to
    /// a float. A non-numeric value raises `TypeError`, so like `as_int`
    /// the conversion runs under `Mrb::protect` and dispatches no `to_f`.
    /// Distinct from `f64::from_value`, the exact-tag downcast that never
    /// converts across types and rejects an Integer outright.
    ///
    /// The converted number round-trips through `Value::from_float` and
    /// `unbox_float`, lossless under beni's pinned float config.
    #[inline]
    pub fn as_float(self, mrb: &Mrb) -> Result<sys::mrb_float, Error> {
        #[cfg(mruby_linked)]
        {
            mrb.protect(|mrb| {
                // SAFETY: as `as_int`; `mrb_as_float` raises `TypeError`
                // on a non-numeric value, caught by `protect`. The result
                // re-boxes losslessly through `from_float`.
                let f = unsafe { sys::mrb_as_float_func(mrb.as_ptr(), self.0) };
                Value::from_float(mrb, f)
            })
            // SAFETY: the `Ok` value was boxed by `Value::from_float`
            // just above, so it carries a Float tag the unbox accepts.
            .map(|v| unsafe { v.unbox_float() })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }
}

/// A non-local `break` / `return` captured as the value inside a
/// protected `Proc::call`'s `Err`. `#[repr(transparent)]` over the
/// break-tagged `Value` it wraps; obtained only through
/// `Value::as_break`.
///
/// Exposes the value the break carries. Classifying the break — a real
/// `break` versus a `return` aimed past a frame — needs mruby's
/// call-info frame indices, which are VM internals reached through the
/// unsafe `beni::sys` escape hatch, not this typed surface.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Break(Value);

impl Break {
    /// The value carried by `break val` / `return val`, via
    /// `mrb_break_value_func`.
    #[inline]
    pub fn value(&self) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.0` is break-tagged by the `Value::as_break`
            // gate that is this newtype's only constructor.
            Value::from_raw(unsafe { sys::mrb_break_value_func(self.0.as_raw()) })
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cstr_macro_appends_nul_terminator() {
        let p = cstr!("hello");
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "hello");
    }

    #[test]
    fn cstr_ptr_accepts_nul_terminated_bytes() {
        const NAME: &[u8] = b"Kobako\0";
        let p = cstr_ptr(NAME);
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "Kobako");
    }

    #[test]
    fn cstr_macro_handles_empty_string() {
        let p = cstr!("");
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "");
    }

    #[test]
    fn value_shares_abi_with_mrb_value() {
        // The `Value` newtype is `#[repr(transparent)]` over
        // `sys::mrb_value`, which is the load-bearing invariant
        // for the `core::mem::transmute(func)` inside
        // `Class::define_method` / `define_singleton_method`
        // (typed `beni::mrb_func_t` → raw `sys::mrb_func_t`).
        // If a future change removes the repr attribute, drops a
        // field, or adds padding, the transmute becomes UB; this
        // test fails first.
        assert_eq!(
            core::mem::size_of::<Value>(),
            core::mem::size_of::<sys::mrb_value>(),
        );
        assert_eq!(
            core::mem::align_of::<Value>(),
            core::mem::align_of::<sys::mrb_value>(),
        );
    }
}

#[cfg(all(test, mruby_linked))]
mod linked_tests {
    use super::*;
    use crate::state::args::format;
    use crate::{Ccontext, Error, FromValue, IntoValue, Module, Proc, Symbol};

    /// Yielder method in the boundary-terminating shape kobako uses:
    /// read the captured (non-orphan) block, yield it, and on a real
    /// `break` report its carried value back as the method's result.
    fn report_break(mrb: &Mrb, _self: Value) -> Value {
        let (_sym, _rest, block_val) = mrb.get_args::<format::NRestBlock>();
        let block = Proc::from_value(block_val).expect("the captured block is a Proc");
        match block.call(mrb, &[]) {
            Ok(_) => Value::from_int(mrb, -1),
            Err(Error::Exception(exc)) => match exc.as_break() {
                Some(brk) => brk.value(),
                None => Value::from_int(mrb, -2),
            },
            Err(Error::Panic(_)) => Value::from_int(mrb, -3),
        }
    }

    #[test]
    fn as_break_rejects_non_break_values() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // No ordinary value carries the break tag — including a real
        // exception object (a raise is not a break).
        assert!(42i32.into_value(&mrb).as_break().is_none());
        assert!(mrb.str_new(b"x").as_value().as_break().is_none());
        assert!(Value::nil().as_break().is_none());
    }

    #[test]
    fn funcall_dispatches_a_method_and_returns_its_value() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // `42.to_s` dispatches `Integer#to_s` and hands back its String.
        let got = 42i32
            .into_value(&mrb)
            .funcall(&mrb, c"to_s", &[])
            .expect("a non-raising dispatch must come back Ok");
        assert_eq!(got.to_string(&mrb), "42");
    }

    #[test]
    fn funcall_passes_the_argument_slice_to_the_method() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // `40 + 2` proves the arg slice reaches the dispatched method.
        let got = 40i32
            .into_value(&mrb)
            .funcall(&mrb, c"+", &[2i32.into_value(&mrb)])
            .expect("a non-raising dispatch must come back Ok");
        assert_eq!(i32::from_value(got), Some(42));
    }

    #[test]
    fn funcall_surfaces_a_raised_exception_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Dispatching an undefined method raises `NoMethodError`, which the
        // protect frame catches into `Err` rather than long-jumping.
        let err = 42i32
            .into_value(&mrb)
            .funcall(&mrb, c"no_such_method", &[])
            .expect_err("a raising dispatch must surface as Err");
        match err {
            Error::Exception(_) => {}
            Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
        }
        // The VM stays usable after the protected raise.
        let again = 7i32
            .into_value(&mrb)
            .funcall(&mrb, c"to_s", &[])
            .expect("the VM must survive the protected raise");
        assert_eq!(again.to_string(&mrb), "7");
    }

    #[test]
    fn funcall_accepts_a_symbol_key_identical_to_the_name() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An interned `Symbol` key reaches the same dispatch as the
        // equivalent name, proving the `IntoSym` generalization routes
        // both through the same interned symbol.
        let recv = 42i32.into_value(&mrb);
        let by_name = recv
            .funcall(&mrb, c"to_s", &[])
            .expect("the name key must dispatch");
        let by_sym = recv
            .funcall(&mrb, Symbol::new(&mrb, c"to_s"), &[])
            .expect("the symbol key must dispatch");
        assert_eq!(by_name.to_string(&mrb), by_sym.to_string(&mrb));
        assert_eq!(by_sym.to_string(&mrb), "42");
    }

    #[test]
    fn funcall_with_block_yields_to_the_passed_block() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A block that records each doubled element into a global array.
        // `Array#each` yields every element to it; reading `$seen` back
        // proves the block reached the dispatched method and ran.
        let cxt = Ccontext::new(&mrb, c"funcall_block.rb")
            .expect("allocating the compile context must succeed");
        cxt.load_nstring(b"$seen = []");
        let block = Proc::from_value(cxt.load_nstring(b"proc { |x| $seen << x * 2 }"))
            .expect("a proc literal carries MRB_TT_PROC");

        let receiver = cxt.load_nstring(b"[1, 2, 3]");
        receiver
            .funcall_with_block(&mrb, c"each", &[], block)
            .expect("yielding through each must come back Ok");

        assert_eq!(cxt.load_nstring(b"$seen").to_string(&mrb), "[2, 4, 6]");
    }

    #[test]
    fn funcall_with_block_surfaces_a_raised_exception_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let cxt = Ccontext::new(&mrb, c"funcall_block_raise.rb")
            .expect("allocating the compile context must succeed");
        let block = Proc::from_value(cxt.load_nstring(b"proc { raise 'boom from block' }"))
            .expect("a proc literal carries MRB_TT_PROC");

        // The block raises while `each` yields to it; the protect frame
        // catches the raise into `Err` rather than long-jumping across FFI.
        let receiver = cxt.load_nstring(b"[1]");
        let err = receiver
            .funcall_with_block(&mrb, c"each", &[], block)
            .expect_err("a raising block must surface as Err");
        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("boom from block")),
            Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
        }

        // The VM stays usable after the protected raise.
        let again = 7i32
            .into_value(&mrb)
            .funcall(&mrb, c"to_s", &[])
            .expect("the VM must survive the protected raise");
        assert_eq!(again.to_string(&mrb), "7");
    }

    #[test]
    fn is_string_discriminates_the_string_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        assert!(mrb.str_new(b"x").as_value().is_string());
        // A non-String tag — and an immediate — both reject.
        assert!(!42i32.into_value(&mrb).is_string());
        assert!(!Value::nil().is_string());
    }

    #[test]
    fn tag_predicates_discriminate_module_range_and_exception() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"tag_pred_test.rb").expect("allocating the context must succeed");

        let module = cxt.load_nstring(b"Enumerable");
        let range = cxt.load_nstring(b"(1..3)");
        let exception = cxt.load_nstring(b"RuntimeError.new('boom')");
        assert!(
            mrb.pending_exc().is_nil(),
            "the literals must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // Each predicate holds for exactly its own tag.
        assert!(module.is_module());
        assert!(range.is_range());
        assert!(exception.is_exception());

        // A class is not a module: is_class and is_module split the
        // class-family tags, and neither claims the other's value.
        let class = cxt.load_nstring(b"String");
        assert!(class.is_class());
        assert!(!class.is_module());
        assert!(!module.is_class());

        // No predicate claims an unrelated tag, nor an immediate.
        assert!(!range.is_module());
        assert!(!exception.is_range());
        assert!(!42i32.into_value(&mrb).is_exception());
        assert!(!Value::nil().is_module());
    }

    #[test]
    fn to_string_reads_a_string_subclass_result() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"to_s_test.rb").expect("allocating the context must succeed");

        // `to_s` returns a String *subclass* instance: String-tagged, so it
        // reads the same way as a plain String. The tag, not the classname,
        // decides — the subclass result converts rather than collapsing to
        // an empty string.
        let obj = cxt.load_nstring(
            b"class BeniSubStr < String; end; class BeniHasSubToS; def to_s; BeniSubStr.new('sub'); end; end; BeniHasSubToS.new",
        );
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the classes must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        assert_eq!(obj.to_string(&mrb), "sub");
    }

    #[test]
    fn inspect_renders_the_ruby_debug_string() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A String inspects quoted, an Integer and nil render canonically —
        // the debug forms, not the to_s forms.
        assert_eq!(mrb.str_new(b"hi").as_value().inspect(&mrb), "\"hi\"");
        assert_eq!(42i32.into_value(&mrb).inspect(&mrb), "42");
        assert_eq!(Value::nil().inspect(&mrb), "nil");
    }

    #[test]
    fn inspect_swallows_a_raising_inspect_as_empty() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"inspect_test.rb")
            .expect("allocating the compile context must succeed");

        let obj = cxt
            .load_nstring(b"class BoomInspect; def inspect; raise 'no'; end; end; BoomInspect.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // A raising user inspect is swallowed into an empty string, and the
        // protect frame leaves no pending exception behind.
        assert_eq!(obj.inspect(&mrb), String::new());
        assert!(
            mrb.pending_exc().is_nil(),
            "the swallowed raise must not leave a pending exception"
        );
    }

    #[test]
    fn any_to_s_renders_the_default_object_form() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"any_to_s_test.rb")
            .expect("allocating the compile context must succeed");

        // A user class with no to_s override renders the default
        // `#<ClassName:0x...>` form, built from the class name without
        // dispatching the receiver's own to_s.
        let obj = cxt.load_nstring(b"class Plain; end; Plain.new");
        let rendered = obj.any_to_s(&mrb).to_bytes();
        assert!(
            rendered.starts_with(b"#<Plain:0x"),
            "expected the default heap-object form, got {:?}",
            String::from_utf8_lossy(&rendered)
        );
        assert!(rendered.ends_with(b">"));
    }

    #[test]
    fn equality_separates_value_eql_and_identity() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let a = mrb.str_new(b"hello").as_value();
        let b = mrb.str_new(b"hello").as_value();
        let c = mrb.str_new(b"world").as_value();

        // `==` and `eql?` are by value: distinct String objects with the
        // same content compare equal, differing content does not.
        assert!(a.equal(&mrb, b).expect("== does not raise for strings"));
        assert!(a.eql(&mrb, b).expect("eql? does not raise for strings"));
        assert!(!a.equal(&mrb, c).expect("== does not raise for strings"));

        // `equal?` is identity: a value is the same object as itself but
        // not as a distinct equal-valued object.
        assert!(a.obj_equal(&mrb, a));
        assert!(!a.obj_equal(&mrb, b));
    }

    #[test]
    fn object_id_is_stable_per_value_and_distinct_across_identity() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let a = mrb.str_new(b"hello").as_value();
        let b = mrb.str_new(b"hello").as_value();

        // The id reads from identity, not content: a value's id equals
        // its own, two identity-distinct objects of equal content differ.
        assert_eq!(a.object_id(), a.object_id());
        assert_ne!(a.object_id(), b.object_id());

        // An immediate's id is likewise its own and stable.
        let n = 7i32.into_value(&mrb);
        assert_eq!(n.object_id(), 7i32.into_value(&mrb).object_id());
    }

    #[test]
    fn equal_surfaces_a_raising_user_method_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"eq_test.rb").expect("allocating the context must succeed");

        let obj = cxt.load_nstring(b"class Boom; def ==(o); raise 'no'; end; end; Boom.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // Comparing dispatches the user `==`, which raises — the raise
        // surfaces as Err instead of unwinding across the call.
        let other = mrb.str_new(b"x").as_value();
        assert!(matches!(obj.equal(&mrb, other), Err(Error::Exception(_))));
    }

    #[test]
    fn eql_surfaces_a_raising_user_method_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"eql_test.rb").expect("allocating the context must succeed");

        let obj =
            cxt.load_nstring(b"class BoomEql; def eql?(o); raise 'no'; end; end; BoomEql.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // `eql?` short-circuits on identity: comparing the object to
        // itself returns true without reaching the raising eql?.
        assert!(matches!(obj.eql(&mrb, obj), Ok(true)));

        // Distinct objects reach the dispatch — which raises, surfacing
        // as Err rather than unwinding across the call, the eql?
        // counterpart to the `==` path above.
        let other = mrb.str_new(b"x").as_value();
        assert!(matches!(obj.eql(&mrb, other), Err(Error::Exception(_))));
    }

    #[test]
    fn cmp_ranks_comparable_values_and_yields_none_for_incomparable() {
        use core::cmp::Ordering;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let one = 1i32.into_value(&mrb);
        let two = 2i32.into_value(&mrb);

        // `<=>` ranks the three orderings.
        assert!(matches!(one.cmp(&mrb, two), Ok(Some(Ordering::Less))));
        assert!(matches!(one.cmp(&mrb, one), Ok(Some(Ordering::Equal))));
        assert!(matches!(two.cmp(&mrb, one), Ok(Some(Ordering::Greater))));

        // Values with no ordering between them — an integer against a
        // string — yield nothing rather than an error.
        let s = mrb.str_new(b"x").as_value();
        assert!(matches!(one.cmp(&mrb, s), Ok(None)));
    }

    #[test]
    fn cmp_ranks_a_custom_spaceship_by_sign_not_magnitude() {
        use core::cmp::Ordering;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"cmp_test.rb").expect("allocating the context must succeed");

        // A `<=>` is only obliged to return negative / zero / positive, so
        // a custom one can answer with any magnitude. `Wide#<=>` reports
        // "greater" as 2 and "less" as -3 to prove ranking keys on sign.
        let greater = cxt.load_nstring(b"class Wide; def <=>(o); 2; end; end; Wide.new");
        let less = cxt.load_nstring(b"class Narrow; def <=>(o); -3; end; end; Narrow.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the classes must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        let other = mrb.str_new(b"x").as_value();
        assert!(matches!(
            greater.cmp(&mrb, other),
            Ok(Some(Ordering::Greater))
        ));
        assert!(matches!(less.cmp(&mrb, other), Ok(Some(Ordering::Less))));
    }

    #[test]
    fn cmp_surfaces_a_raising_user_method_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"cmp_test.rb").expect("allocating the context must succeed");

        let obj = cxt.load_nstring(b"class BoomCmp; def <=>(o); raise 'no'; end; end; BoomCmp.new");
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // Comparing dispatches the user `<=>`, which raises — the raise
        // surfaces as Err rather than unwinding across the call.
        let other = mrb.str_new(b"x").as_value();
        assert!(matches!(obj.cmp(&mrb, other), Err(Error::Exception(_))));
    }

    #[test]
    fn dup_and_clone_surface_a_raising_initialize_copy_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"copy_test.rb").expect("allocating the context must succeed");

        let obj = cxt.load_nstring(
            b"class BoomCopy; def initialize_copy(o); raise 'no'; end; end; BoomCopy.new",
        );
        assert!(
            mrb.pending_exc().is_nil(),
            "defining the class must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // Both copies run `initialize_copy`, which raises — surfaced as
        // Err instead of unwinding across the call.
        assert!(matches!(obj.obj_dup(&mrb), Err(Error::Exception(_))));
        assert!(matches!(obj.obj_clone(&mrb), Err(Error::Exception(_))));
    }

    #[test]
    fn check_frozen_guards_frozen_and_immediate_receivers() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A mutable heap object passes the guard.
        let mutable = mrb.str_new(b"open").as_value();
        assert!(matches!(mutable.check_frozen(&mrb), Ok(())));

        // Freezing it flips the guard to a `FrozenError`, surfaced as Err
        // rather than unwinding across the call. The protect frame leaves
        // no pending exception behind.
        let frozen = mutable.freeze(&mrb);
        let err = frozen
            .check_frozen(&mrb)
            .expect_err("a frozen receiver must surface as Err");
        match err {
            Error::Exception(exc) => {
                assert_eq!(exc.classname(&mrb), "FrozenError");
            }
            Error::Panic(_) => unreachable!("the guard must surface as Error::Exception"),
        }
        assert!(
            mrb.pending_exc().is_nil(),
            "the caught raise must not leave a pending exception"
        );

        // An immediate counts as frozen.
        assert!(matches!(
            42i32.into_value(&mrb).check_frozen(&mrb),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn obj_as_string_coerces_through_to_s() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Already a string: coercion returns that same string, not a copy.
        let already = mrb.str_new(b"hi").as_value();
        let coerced = already
            .obj_as_string(&mrb)
            .expect("a string coerces without raising");
        assert!(coerced.is_string());
        assert!(already.obj_equal(&mrb, coerced));

        // A non-string coerces through its `to_s`.
        assert!(42i32
            .into_value(&mrb)
            .obj_as_string(&mrb)
            .expect("to_s of an integer does not raise")
            .is_string());
    }

    #[test]
    fn ensure_string_returns_the_handle_or_raises_by_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A String tag yields the same string as a typed handle — no
        // copy, no dispatch.
        let s = mrb.str_new(b"hi").as_value();
        let handle = s
            .ensure_string(&mrb)
            .expect("a String value coerces without raising");
        assert!(s.obj_equal(&mrb, handle.as_value()));
        assert_eq!(handle.to_bytes(), b"hi".to_vec());

        // A non-String tag raises `TypeError` rather than coercing — the
        // contrast with `obj_as_string`, which would render the integer
        // through `to_s`. The raise is the genuine `TypeError` class, not
        // some other exception.
        match 42i32.into_value(&mrb).ensure_string(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-String value surfaces a TypeError Err"),
        }
    }

    #[test]
    fn ensure_array_returns_the_handle_or_raises_by_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An Array tag yields the same array as a typed handle — no
        // copy, no dispatch.
        let a = mrb.ary_new().as_value();
        let handle = a
            .ensure_array(&mrb)
            .expect("an Array value coerces without raising");
        assert!(a.obj_equal(&mrb, handle.as_value()));

        // A non-Array tag raises `TypeError` rather than coercing — no
        // `to_ary` dispatch. The raise is the genuine `TypeError`
        // class, not some other exception.
        match 42i32.into_value(&mrb).ensure_array(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-Array value surfaces a TypeError Err"),
        }
    }

    /// A `to_a` that returns a non-array non-`nil` value — the case that
    /// makes `mrb_ary_splat` raise rather than wrap.
    fn to_a_returns_int(_mrb: &Mrb, _self: Value) -> i32 {
        42
    }

    /// A `to_a` that returns `nil` — the case `mrb_ary_splat` wraps in a
    /// one-element array holding the receiver.
    fn to_a_returns_nil(_mrb: &Mrb, _self: Value) -> Value {
        Value::nil()
    }

    #[test]
    fn to_ary_spreads_or_wraps_each_value_kind() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An array spreads to a copy: same elements, distinct object.
        let src = mrb.ary_new_from_values(&[1i32.into_value(&mrb), 2i32.into_value(&mrb)]);
        let spread = src
            .as_value()
            .to_ary(&mrb)
            .expect("an array spreads without raising");
        assert_eq!(spread.len(), 2);
        assert!(!src.as_value().obj_equal(&mrb, spread.as_value()));

        // A scalar that does not respond to `to_a` wraps in `[scalar]`.
        let wrapped = 7i32
            .into_value(&mrb)
            .to_ary(&mrb)
            .expect("a scalar wraps without raising");
        assert_eq!(wrapped.len(), 1);
        assert_eq!(i32::from_value(wrapped.entry(0)), Some(7));

        // `nil` answers `to_a` with an empty array here (mruby-object-ext
        // defines `NilClass#to_a`), so it spreads to `[]` — the responder
        // path, not a wrap.
        let nil_spread = Value::nil()
            .to_ary(&mrb)
            .expect("nil spreads through its to_a");
        assert_eq!(nil_spread.len(), 0);

        // A `to_a` responder whose result is an array passes that array
        // through: a Range yields its enumerated elements.
        let range = mrb
            .range_new(1i32.into_value(&mrb), 3i32.into_value(&mrb), false)
            .expect("a Range over comparable bounds constructs");
        let enumerated = range
            .as_value()
            .to_ary(&mrb)
            .expect("a Range spreads through its to_a");
        assert_eq!(enumerated.len(), 3);

        // A `to_a` that returns `nil` falls back to wrapping the receiver
        // in a one-element array.
        let nil_class = mrb
            .class_new(mrb.object_class())
            .expect("an anonymous class under Object constructs");
        nil_class
            .define_method(&mrb, c"to_a", crate::method!(to_a_returns_nil, 0))
            .expect("registering to_a must succeed");
        let nil_obj = nil_class
            .obj_new(&mrb, &[])
            .expect("the class whose to_a returns nil instantiates");
        let nil_returned = nil_obj
            .to_ary(&mrb)
            .expect("a nil-returning to_a wraps without raising");
        assert_eq!(nil_returned.len(), 1);
        assert!(nil_obj.obj_equal(&mrb, nil_returned.entry(0)));

        // A `to_a` that returns a non-array non-`nil` value raises a
        // genuine `TypeError`, caught into the `Err` rather than wrapping.
        let class = mrb
            .class_new(mrb.object_class())
            .expect("an anonymous class under Object constructs");
        class
            .define_method(&mrb, c"to_a", crate::method!(to_a_returns_int, 0))
            .expect("registering to_a must succeed");
        let obj = class
            .obj_new(&mrb, &[])
            .expect("the class with a misbehaving to_a instantiates");
        match obj.to_ary(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-array non-nil to_a surfaces a TypeError Err"),
        }
    }

    #[test]
    fn ensure_hash_returns_the_handle_or_raises_by_tag() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A Hash tag yields the same hash as a typed handle — no copy,
        // no dispatch.
        let h = mrb.hash_new().as_value();
        let handle = h
            .ensure_hash(&mrb)
            .expect("a Hash value coerces without raising");
        assert!(h.obj_equal(&mrb, handle.as_value()));

        // A non-Hash tag raises `TypeError` rather than coercing — no
        // `to_hash` dispatch. The raise is the genuine `TypeError`
        // class, not some other exception.
        match 42i32.into_value(&mrb).ensure_hash(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-Hash value surfaces a TypeError Err"),
        }
    }

    #[test]
    fn bool_predicates_separate_true_false_and_nil() {
        // The immediate singletons need a live VM to have been captured,
        // even though the predicates themselves take no `Mrb`.
        let _mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // `is_true` / `is_false` are exact: each admits only its own
        // singleton. The load-bearing case is that `nil` — which shares
        // the false tag under some boxing modes — is neither.
        assert!(Value::true_().is_true());
        assert!(!Value::true_().is_false());
        assert!(Value::false_().is_false());
        assert!(!Value::false_().is_true());
        assert!(!Value::nil().is_true());
        assert!(!Value::nil().is_false());
    }

    #[test]
    fn to_bool_follows_ruby_truthiness() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Only `nil` and `false` are falsy; every other value — zero
        // and the empty string included — is truthy.
        assert!(Value::true_().to_bool());
        assert!(!Value::false_().to_bool());
        assert!(!Value::nil().to_bool());
        assert!(0i32.into_value(&mrb).to_bool());
        assert!(mrb.str_new(b"").as_value().to_bool());
    }

    #[test]
    fn obj_dup_copies_state_into_an_independent_object() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"dup_test.rb")
            .expect("allocating the compile context must succeed");

        let orig = cxt.load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        let dup = orig.obj_dup(&mrb).expect("dup does not raise");
        let x = mrb.intern_cstr(c"@x");
        // The dup carries the copied ivar...
        assert_eq!(i32::from_value(dup.iv_get(&mrb, x)), Some(1));
        // ...and is a distinct object: mutating it leaves the original.
        dup.iv_set(&mrb, x, 2i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        assert_eq!(i32::from_value(dup.iv_get(&mrb, x)), Some(2));
        assert_eq!(i32::from_value(orig.iv_get(&mrb, x)), Some(1));
    }

    #[test]
    fn iv_set_surfaces_frozen_and_non_object_receivers_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"iv_set_test.rb").expect("allocating the context must succeed");
        let x = mrb.intern_cstr(c"@x");
        let one = 1i32.into_value(&mrb);

        // A frozen receiver rejects the assignment — surfaced as Err
        // instead of unwinding across the call.
        let frozen = cxt.load_nstring(b"Object.new.freeze");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");
        assert!(matches!(
            frozen.iv_set(&mrb, x, one),
            Err(Error::Exception(_))
        ));

        // An immediate cannot hold instance variables — also an Err, not UB.
        assert!(matches!(
            42i32.into_value(&mrb).iv_set(&mrb, x, one),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn const_get_reads_a_constant_and_surfaces_an_absent_one_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"const_get_test.rb").expect("allocating the context must succeed");

        let module = cxt.load_nstring(b"module BeniConstHost; FOO = 7; end; BeniConstHost");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A defined constant reads back its value.
        let foo = mrb.intern_cstr(c"FOO");
        assert_eq!(
            i32::from_value(module.const_get(&mrb, foo).expect("FOO is defined")),
            Some(7)
        );

        // An absent constant raises NameError — surfaced as Err instead
        // of unwinding across the call.
        let missing = mrb.intern_cstr(c"BENI_MISSING");
        assert!(matches!(
            module.const_get(&mrb, missing),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn cv_get_reads_a_class_variable_and_surfaces_an_absent_one_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"cv_get_test.rb").expect("allocating the context must succeed");

        let class = cxt.load_nstring(b"class BeniCvHost; @@count = 3; end; BeniCvHost");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A defined class variable reads back its value.
        let count = mrb.intern_cstr(c"@@count");
        assert_eq!(
            i32::from_value(class.cv_get(&mrb, count).expect("@@count is defined")),
            Some(3)
        );

        // An absent class variable raises NameError — surfaced as Err
        // instead of unwinding across the call.
        let missing = mrb.intern_cstr(c"@@beni_missing");
        assert!(matches!(
            class.cv_get(&mrb, missing),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn const_set_assigns_a_constant_and_surfaces_a_non_module_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"const_set_test.rb").expect("allocating the context must succeed");

        let module = cxt.load_nstring(b"module BeniConstWriteHost; end; BeniConstWriteHost");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A fresh constant assigned on a module reads back its value.
        let bar = mrb.intern_cstr(c"BAR");
        module
            .const_set(&mrb, bar, 9i32.into_value(&mrb))
            .expect("assigning a constant on a module must succeed");
        assert_eq!(
            i32::from_value(module.const_get(&mrb, bar).expect("BAR was just set")),
            Some(9)
        );

        // A non-module receiver raises TypeError — surfaced as Err
        // instead of unwinding across the call.
        assert!(matches!(
            42i32.into_value(&mrb).const_set(&mrb, bar, Value::nil()),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn const_remove_removes_a_constant_and_surfaces_a_non_module_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"const_remove_test.rb")
            .expect("allocating the context must succeed");

        let module =
            cxt.load_nstring(b"module BeniConstRemoveHost; GONE = 5; end; BeniConstRemoveHost");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // Removing a defined constant succeeds and clears its presence.
        let gone = mrb.intern_cstr(c"GONE");
        assert!(module.const_defined(&mrb, gone), "GONE is defined");
        module
            .const_remove(&mrb, gone)
            .expect("removing a defined constant must succeed");
        assert!(
            !module.const_defined(&mrb, gone),
            "GONE is gone after removal"
        );

        // Removing an absent constant is a no-op, not an error.
        module
            .const_remove(&mrb, gone)
            .expect("removing an absent constant is a no-op");

        // A non-module receiver raises TypeError — surfaced as Err
        // instead of unwinding across the call.
        assert!(matches!(
            42i32.into_value(&mrb).const_remove(&mrb, gone),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn const_defined_at_answers_only_for_the_receivers_own_constant() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"const_defined_at_test.rb")
            .expect("allocating the context must succeed");

        let child = cxt.load_nstring(
            b"class BeniConstAtParent; OWNED = 1; end; \
              class BeniConstAtChild < BeniConstAtParent; end; BeniConstAtChild",
        );
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        let owned = mrb.intern_cstr(c"OWNED");
        let absent = mrb.intern_cstr(c"ABSENT");

        // A constant living only on the parent walks into reach for the
        // ancestry-walking test but stays invisible to the direct test.
        assert!(
            child.const_defined(&mrb, owned),
            "OWNED is reachable through the ancestry"
        );
        assert!(
            !child.const_defined_at(&mrb, owned),
            "OWNED is inherited, not on the child's own table"
        );

        // The constant on the receiver's own table is seen by the direct test.
        let parent = cxt.load_nstring(b"BeniConstAtParent");
        assert!(
            parent.const_defined_at(&mrb, owned),
            "OWNED is on the parent's own table"
        );

        // An absent constant is false either way — a total predicate.
        assert!(!child.const_defined_at(&mrb, absent), "ABSENT is undefined");
    }

    #[test]
    fn cv_set_assigns_a_class_variable_and_surfaces_a_frozen_receiver_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"cv_set_test.rb").expect("allocating the context must succeed");

        let class = cxt.load_nstring(b"class BeniCvWriteHost; end; BeniCvWriteHost");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A class variable assigned on a class reads back its value.
        let total = mrb.intern_cstr(c"@@total");
        class
            .cv_set(&mrb, total, 5i32.into_value(&mrb))
            .expect("assigning a class variable on a class must succeed");
        assert_eq!(
            i32::from_value(class.cv_get(&mrb, total).expect("@@total was just set")),
            Some(5)
        );

        // A frozen receiver rejects the assignment — surfaced as Err
        // instead of unwinding across the call.
        let frozen = cxt.load_nstring(b"BeniCvWriteHost.freeze");
        assert!(mrb.pending_exc().is_nil(), "freezing must not raise");
        assert!(matches!(
            frozen.cv_set(&mrb, total, 6i32.into_value(&mrb)),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn cv_defined_tests_class_variable_presence_walking_the_ancestry() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"cv_defined_test.rb")
            .expect("allocating the context must succeed");

        let child = cxt.load_nstring(
            b"class BeniCvParent; @@inherited = 1; end; \
              class BeniCvChild < BeniCvParent; end; BeniCvChild",
        );
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A class variable defined on an ancestor is present on the
        // child; an absent one is not — the predicate is total, raising
        // for neither.
        let inherited = mrb.intern_cstr(c"@@inherited");
        let missing = mrb.intern_cstr(c"@@missing");
        assert!(child.cv_defined(&mrb, inherited));
        assert!(!child.cv_defined(&mrb, missing));
    }

    #[test]
    fn iv_defined_tests_instance_variable_presence() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"iv_defined_test.rb")
            .expect("allocating the context must succeed");

        let obj = cxt.load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // A set instance variable is present; an unset one is not — the
        // predicate is total, raising for neither.
        let x = mrb.intern_cstr(c"@x");
        let y = mrb.intern_cstr(c"@y");
        assert!(obj.iv_defined(&mrb, x));
        assert!(!obj.iv_defined(&mrb, y));
    }

    #[test]
    fn iv_remove_yields_the_former_value_and_clears_presence() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"iv_remove_test.rb").expect("allocating the context must succeed");

        let obj = cxt.load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // Removing a set variable hands back its former value and leaves
        // the variable undefined.
        let x = mrb.intern_cstr(c"@x");
        let removed = obj.iv_remove(&mrb, x).expect("removal does not raise");
        assert_eq!(removed.and_then(i32::from_value), Some(1));
        assert!(!obj.iv_defined(&mrb, x));
    }

    #[test]
    fn iv_remove_distinguishes_absent_from_a_removed_nil() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"iv_remove_absent_test.rb")
            .expect("allocating the context must succeed");

        let obj = cxt.load_nstring(b"o = Object.new; o.instance_variable_set(:@x, nil); o");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // An absent variable yields None — distinct from a variable that
        // held nil, which yields Some(nil).
        let y = mrb.intern_cstr(c"@y");
        assert!(obj
            .iv_remove(&mrb, y)
            .expect("absent removal does not raise")
            .is_none());

        let x = mrb.intern_cstr(c"@x");
        let removed = obj.iv_remove(&mrb, x).expect("removal does not raise");
        assert!(removed.is_some_and(Value::is_nil));

        // An immediate cannot hold instance variables — also None, not Err.
        assert!(42i32
            .into_value(&mrb)
            .iv_remove(&mrb, x)
            .expect("a non-holder removal does not raise")
            .is_none());
    }

    #[test]
    fn iv_remove_surfaces_a_frozen_holder_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"iv_remove_frozen_test.rb")
            .expect("allocating the context must succeed");

        // A frozen instance-variable holder rejects removal — surfaced as
        // Err instead of unwinding across the call.
        let frozen =
            cxt.load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o.freeze; o");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");
        let x = mrb.intern_cstr(c"@x");
        assert!(matches!(
            frozen.iv_remove(&mrb, x),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn obj_clone_carries_frozen_state_where_dup_drops_it() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"clone_test.rb")
            .expect("allocating the compile context must succeed");

        let frozen = cxt.load_nstring(b"Object.new.freeze");
        assert!(mrb.pending_exc().is_nil(), "setup must not raise");

        // clone is the deeper copy — it preserves the frozen state;
        // dup always yields an unfrozen object.
        assert!(frozen
            .obj_clone(&mrb)
            .expect("clone does not raise")
            .funcall(&mrb, c"frozen?", &[])
            .expect("frozen? does not raise")
            .to_bool());
        assert!(!frozen
            .obj_dup(&mrb)
            .expect("dup does not raise")
            .funcall(&mrb, c"frozen?", &[])
            .expect("frozen? does not raise")
            .to_bool());
    }

    #[test]
    fn as_break_views_a_real_escaping_break() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let class = mrb
            .define_class(c"BeniBreakYielder", mrb.object_class())
            .expect("defining the yielder class must succeed");
        class
            .define_method(&mrb, c"run", crate::method!(report_break, -1))
            .expect("registering the yielder method must succeed");

        let recv = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let slot = mrb.intern_cstr(c"$beni_break_recv");
        mrb.gv_set(slot, recv);

        // The block is captured via `&` so it stays non-orphan: `break
        // 88` surfaces as an RBreak the yielder catches, and `as_break`
        // reads its carried value back out.
        let cxt = Ccontext::new(&mrb, c"break_test.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"$beni_break_recv.run(:tag) { break 88 }");

        assert!(
            mrb.pending_exc().is_nil(),
            "the protected yield must not leave a pending exception: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(i32::from_value(got), Some(88));
    }

    #[test]
    fn class_and_kind_predicates_read_the_hierarchy() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let s = mrb.str_new(b"hi").as_value();
        let string_class = mrb.class_get(c"String").expect("String is defined");
        let object_class = mrb.class_get(c"Object").expect("Object is defined");

        // class() names the receiver's direct class.
        assert_eq!(s.class(&mrb).name(&mrb), "String");

        // is_kind_of holds for the direct class and its ancestors;
        // instance_of only for the direct class.
        assert!(s.is_kind_of(&mrb, string_class));
        assert!(s.is_kind_of(&mrb, object_class));
        assert!(s.is_instance_of(&mrb, string_class));
        assert!(!s.is_instance_of(&mrb, object_class));
    }

    #[test]
    fn singleton_class_reads_a_stable_eigenclass_and_rejects_immediates() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let s = mrb.str_new(b"hi").as_value();

        // An ordinary object's singleton class is its own per-instance
        // eigenclass — distinct from the regular class it shares with peers.
        let sclass = s
            .singleton_class(&mrb)
            .expect("a string has a singleton class");
        assert_ne!(sclass.as_raw(), s.class(&mrb).as_raw());

        // Re-reading the same object yields the same singleton class.
        let again = s
            .singleton_class(&mrb)
            .expect("a string has a singleton class");
        assert_eq!(sclass.as_raw(), again.as_raw());

        // nil yields its predefined class, which acts as its singleton
        // class, so the read succeeds.
        assert_eq!(
            Value::nil()
                .singleton_class(&mrb)
                .expect("nil has a singleton class")
                .as_raw(),
            Value::nil().class(&mrb).as_raw()
        );

        // Every other immediate has no singleton class: the TypeError
        // mruby raises surfaces as Err.
        match Value::from_int(&mrb, 1).singleton_class(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            other => panic!("expected a TypeError Err, got {other:?}"),
        }
    }

    #[test]
    fn freeze_marks_the_value_frozen() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let s = mrb.str_new(b"x").as_value();
        assert!(!s
            .funcall(&mrb, c"frozen?", &[])
            .expect("frozen? does not raise")
            .to_bool());

        let frozen = s.freeze(&mrb);
        assert!(frozen
            .funcall(&mrb, c"frozen?", &[])
            .expect("frozen? does not raise")
            .to_bool());
    }

    #[test]
    fn as_int_converts_across_numeric_types_and_surfaces_non_numeric_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An Integer reads directly.
        assert_eq!(
            42i32
                .into_value(&mrb)
                .as_int(&mrb)
                .expect("an Integer converts"),
            42
        );

        // A Float converts by truncating toward zero — unlike the
        // exact-tag `i32::from_value`, which rejects the Float tag.
        let float_val = 2.9f64.into_value(&mrb);
        assert_eq!(i32::from_value(float_val), None);
        assert_eq!(float_val.as_int(&mrb).expect("a Float truncates"), 2);

        // A non-numeric value raises TypeError — surfaced as Err instead
        // of unwinding across the call.
        assert!(matches!(
            mrb.str_new(b"x").as_value().as_int(&mrb),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn as_float_converts_across_numeric_types_and_surfaces_non_numeric_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A Float reads directly.
        assert_eq!(
            1.5f64
                .into_value(&mrb)
                .as_float(&mrb)
                .expect("a Float converts"),
            1.5
        );

        // An Integer widens to a float — unlike the exact-tag
        // `f64::from_value`, which rejects the Integer tag.
        let int_val = 3i32.into_value(&mrb);
        assert_eq!(f64::from_value(int_val), None);
        assert_eq!(int_val.as_float(&mrb).expect("an Integer widens"), 3.0);

        // A non-numeric value raises TypeError — surfaced as Err.
        assert!(matches!(
            mrb.str_new(b"x").as_value().as_float(&mrb),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn int_to_str_renders_in_base_ten_and_other_radixes() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let n = Value::from_int(&mrb, 12345);
        // Base 10 is the plain decimal rendering.
        assert_eq!(
            n.int_to_str(&mrb, 10).expect("base 10 renders").to_bytes(),
            b"12345".to_vec()
        );
        // A non-decimal radix renders in that base, like Ruby's
        // 12345.to_s(16) == "3039".
        assert_eq!(
            n.int_to_str(&mrb, 16).expect("base 16 renders").to_bytes(),
            b"3039".to_vec()
        );
    }

    #[test]
    fn int_to_str_surfaces_an_invalid_radix_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A radix outside 2 through 36 raises ArgumentError, caught into
        // Err rather than long-jumping; the VM stays usable afterward.
        assert!(matches!(
            Value::from_int(&mrb, 12345).int_to_str(&mrb, 1),
            Err(Error::Exception(_))
        ));
        assert_eq!(
            Value::from_int(&mrb, 42)
                .int_to_str(&mrb, 10)
                .expect("the VM survives the protected raise")
                .to_bytes(),
            b"42".to_vec()
        );
    }

    #[test]
    fn int_to_str_rejects_a_non_integer_receiver() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // The guard is strict on the Integer tag: a Float is rejected with
        // TypeError, not coerced, because mrb_integer_to_str unboxes its
        // receiver without a tag check.
        assert!(matches!(
            1.5f64.into_value(&mrb).int_to_str(&mrb, 10),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn float_to_int_truncates_toward_zero() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A positive float truncates down, like Ruby's 3.9.to_i == 3.
        let three = Value::from_float(&mrb, 3.9)
            .float_to_int(&mrb)
            .expect("3.9 converts");
        assert_eq!(i32::from_value(three), Some(3));
        // A negative float truncates toward zero, like Ruby's -3.9.to_i == -3.
        let neg_three = Value::from_float(&mrb, -3.9)
            .float_to_int(&mrb)
            .expect("-3.9 converts");
        assert_eq!(i32::from_value(neg_three), Some(-3));
    }

    #[test]
    fn float_to_int_surfaces_infinity_and_nan_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Infinity and NaN have no integer; mruby raises RangeError, caught
        // into Err rather than long-jumping, and the VM stays usable after.
        assert!(matches!(
            Value::from_float(&mrb, f64::INFINITY).float_to_int(&mrb),
            Err(Error::Exception(_))
        ));
        assert!(matches!(
            Value::from_float(&mrb, f64::NAN).float_to_int(&mrb),
            Err(Error::Exception(_))
        ));
        assert_eq!(
            i32::from_value(
                Value::from_float(&mrb, 2.5)
                    .float_to_int(&mrb)
                    .expect("the VM survives the protected raise")
            ),
            Some(2)
        );
    }

    #[test]
    fn float_to_int_rejects_a_non_float_receiver() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // mrb_float_to_integer guards its receiver on the Float tag: an
        // Integer is rejected with TypeError, not passed through.
        assert!(matches!(
            Value::from_int(&mrb, 7).float_to_int(&mrb),
            Err(Error::Exception(_))
        ));
    }

    #[test]
    fn ensure_int_coerces_by_numeric_type_or_raises() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An Integer coerces unchanged, staying an Integer value.
        let same = Value::from_int(&mrb, 5)
            .ensure_int(&mrb)
            .expect("an Integer coerces without raising");
        assert!(same.is_integer());
        assert_eq!(i32::from_value(same), Some(5));

        // A Float coerces by truncating toward zero, like Ruby's
        // Integer(-3.9) == -3 — the cross-numeric case.
        let truncated = Value::from_float(&mrb, -3.9)
            .ensure_int(&mrb)
            .expect("a Float coerces by truncation");
        assert!(truncated.is_integer());
        assert_eq!(i32::from_value(truncated), Some(-3));

        // An infinite or NaN Float has no integer; mruby raises RangeError,
        // caught into Err, and the VM stays usable.
        assert!(matches!(
            Value::from_float(&mrb, f64::INFINITY).ensure_int(&mrb),
            Err(Error::Exception(_))
        ));

        // A non-numeric value raises the genuine TypeError class rather than
        // coercing — no to_int dispatch.
        match mrb.str_new(b"7").as_value().ensure_int(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-numeric value surfaces a TypeError Err"),
        }
    }

    #[test]
    fn ensure_float_coerces_by_numeric_type_or_raises() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A Float coerces unchanged, staying a Float value.
        let same = Value::from_float(&mrb, 2.5)
            .ensure_float(&mrb)
            .expect("a Float coerces without raising");
        assert!(same.is_float());
        assert_eq!(f64::from_value(same), Some(2.5));

        // An Integer widens to a Float — the cross-numeric case.
        let widened = Value::from_int(&mrb, 7)
            .ensure_float(&mrb)
            .expect("an Integer widens to a Float");
        assert!(widened.is_float());
        assert_eq!(f64::from_value(widened), Some(7.0));

        // A non-numeric value raises the genuine TypeError class rather than
        // coercing — no to_f dispatch.
        match mrb.str_new(b"2.5").as_value().ensure_float(&mrb) {
            Err(Error::Exception(exc)) => {
                assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
            }
            _ => panic!("a non-numeric value surfaces a TypeError Err"),
        }
    }

    #[test]
    fn arithmetic_computes_on_integers_and_floats() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // Integer operands yield an Integer result, like Ruby's 2 + 3 == 5.
        let sum = Value::from_int(&mrb, 2)
            .add(&mrb, Value::from_int(&mrb, 3))
            .expect("2 + 3 computes");
        assert_eq!(i32::from_value(sum), Some(5));
        // Subtraction and multiplication follow the same Integer path.
        let diff = Value::from_int(&mrb, 10)
            .sub(&mrb, Value::from_int(&mrb, 4))
            .expect("10 - 4 computes");
        assert_eq!(i32::from_value(diff), Some(6));
        let product = Value::from_int(&mrb, 6)
            .mul(&mrb, Value::from_int(&mrb, 7))
            .expect("6 * 7 computes");
        assert_eq!(i32::from_value(product), Some(42));
    }

    #[test]
    fn arithmetic_widens_a_mixed_operand_to_float() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // A float operand widens the result to a Float, like Ruby's
        // 2 + 3.5 == 5.5; f64::from_value reads only the Float tag, so a Some
        // confirms the result is a Float, not an Integer.
        let sum = Value::from_int(&mrb, 2)
            .add(&mrb, Value::from_float(&mrb, 3.5))
            .expect("2 + 3.5 computes");
        assert_eq!(f64::from_value(sum), Some(5.5));
        // The float receiver path widens the same way.
        let product = Value::from_float(&mrb, 1.5)
            .mul(&mrb, Value::from_int(&mrb, 4))
            .expect("1.5 * 4 computes");
        assert_eq!(f64::from_value(product), Some(6.0));
    }

    #[test]
    fn arithmetic_rejects_a_non_numeric_operand() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // mrb_num_add dispatches on the numeric tag: a non-numeric right
        // operand raises TypeError, caught into Err rather than long-jumping.
        assert!(matches!(
            Value::from_int(&mrb, 1).add(&mrb, Value::nil()),
            Err(Error::Exception(_))
        ));
        // A non-numeric receiver is rejected the same way.
        assert!(matches!(
            Value::nil().add(&mrb, Value::from_int(&mrb, 1)),
            Err(Error::Exception(_))
        ));
        // The VM stays usable after the protected raise.
        assert_eq!(
            i32::from_value(
                Value::from_int(&mrb, 1)
                    .add(&mrb, Value::from_int(&mrb, 1))
                    .expect("the VM survives the protected raise")
            ),
            Some(2)
        );
    }

    #[test]
    fn arithmetic_surfaces_integer_overflow_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An integer result past the configured width has two lawful
        // outcomes, branched on the build's integer model rather than a
        // compile-time flag: a fixed-width config (no bigint) raises
        // RangeError, caught into Err; a bigint config promotes the result to
        // a BigInt and returns it. The bound is read from sys::mrb_int so the
        // overflow is forced at any width.
        let max = Value::from_int(&mrb, sys::mrb_int::MAX);
        match max.add(&mrb, Value::from_int(&mrb, 1)) {
            Err(Error::Exception(exc)) => {
                // The fixed-width lane stays strict: the surfaced exception is
                // exactly the RangeError the SPEC mandates for that config.
                assert_eq!(exc.classname(&mrb), "RangeError");
            }
            Ok(promoted) => {
                // The bigint lane lawfully promotes instead of raising; the
                // result keeps the Integer class (a BigInt is allocated on
                // mruby's integer_class), so the value stays a numeric
                // Integer rather than degrading to another type.
                assert_eq!(promoted.classname(&mrb), "Integer");
            }
            Err(other) => panic!("overflow must surface as a RangeError, got {other:?}"),
        }
    }

    #[test]
    fn each_iv_visits_every_set_instance_variable() {
        use crate::{ForEach, Symbol};

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt = Ccontext::new(&mrb, c"each_iv.rb").expect("allocating the context must succeed");
        let obj = cxt.load_nstring(b"Object.new");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@c"), 3i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");

        let mut seen = Vec::new();
        obj.each_iv(&mrb, |name: Symbol, val| {
            seen.push((
                name.name(&mrb)
                    .expect("an ivar name interns to a name")
                    .to_owned(),
                i32::from_value(val).expect("the seeded values are integers"),
            ));
            ForEach::Continue
        });
        seen.sort();

        assert_eq!(
            seen,
            vec![
                ("@a".to_owned(), 1),
                ("@b".to_owned(), 2),
                ("@c".to_owned(), 3),
            ]
        );
    }

    #[test]
    fn each_iv_visits_nothing_for_a_receiver_without_instance_variables() {
        use crate::ForEach;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // An immediate cannot hold instance variables, so the guarded
        // foreach returns without ever calling back.
        let mut count = 0;
        Value::from_int(&mrb, 42).each_iv(&mrb, |_, _| {
            count += 1;
            ForEach::Continue
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn each_iv_stops_early_on_stop() {
        use crate::ForEach;

        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"each_iv_stop.rb").expect("allocating the context must succeed");
        let obj = cxt.load_nstring(b"Object.new");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@c"), 3i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");

        // Stopping at the first variable leaves the rest unvisited.
        let mut count = 0;
        obj.each_iv(&mrb, |_, _| {
            count += 1;
            ForEach::Stop
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn each_iv_resurfaces_a_closure_panic_on_the_rust_side() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let cxt =
            Ccontext::new(&mrb, c"each_iv_panic.rb").expect("allocating the context must succeed");
        let obj = cxt.load_nstring(b"Object.new");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");
        obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
            .expect("iv_set on a fresh object does not raise");

        // A panic in the closure is caught at the FFI boundary, stops the
        // walk, and resumes here once mrb_iv_foreach returns — never
        // unwinding through mruby's C frames. catch_unwind sees the
        // resumed panic, proving it crossed back to the Rust side intact.
        let visited = std::cell::Cell::new(0u32);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            obj.each_iv(&mrb, |_, _| {
                visited.set(visited.get() + 1);
                panic!("boom in each_iv closure");
            });
        }));

        let payload = result.expect_err("the closure panic must resurface Rust-side");
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .expect("the original panic payload survives the round-trip");
        assert_eq!(msg, "boom in each_iv closure");
        // The walk stopped at the first variable rather than running on.
        assert_eq!(visited.get(), 1);

        // The VM survives the caught panic.
        assert_eq!(
            i32::from_value(obj.iv_get(&mrb, mrb.intern_cstr(c"@b"))),
            Some(2)
        );
    }

    #[test]
    fn classname_survives_a_gc_cycle() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        // `classname` owns its bytes: mruby builds the name into a
        // GC-managed temporary, so a name held across a collection must
        // keep reading correctly rather than dangle into freed storage.
        let name = mrb.str_new(b"hello").as_value().classname(&mrb);
        mrb.full_gc();
        assert_eq!(name, "String");
    }
}
