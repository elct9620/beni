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
//! `Value` is `#[repr(transparent)]` over `mrb_value`. Under word
//! boxing — mruby's fallback when a config names no boxing mode
//! (`vendor/mruby/include/mrbconf.h:62-64`) — `mrb_value` is a single
//! machine word (4 bytes on wasm32, 8 on 64-bit hosts); `Value` shares
//! that layout and the C ABI. This matters at the `mrb_func_t` boundary:
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

struct Immediates {
    qnil: sys::mrb_value,
    qtrue: sys::mrb_value,
    qfalse: sys::mrb_value,
}

// SAFETY: `mrb_value` under word boxing is a `#[repr(C)]` struct
// holding a single integer word — plain old data with no interior
// mutability. `Immediates` therefore shares only `Copy` snapshots,
// which is sound to read from any thread.
unsafe impl Sync for Immediates {}

static IMMEDIATES: std::sync::OnceLock<Immediates> = std::sync::OnceLock::new();

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
/// ## ABI invariant
///
/// `value::tests::value_shares_abi_with_mrb_value` here and
/// `surface_test::typed_mrb_func_t_coerces_from_value_bridge` in
/// `beni-tests` pin the `#[repr(transparent)]` contract that
/// `Class::define_method`'s `mem::transmute` depends on.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub(crate) sys::mrb_value);

// SAFETY: a handle carries neither the interpreter nor ownership of what
// it names, so crossing a thread with one is inert; it means something
// only against the interpreter that produced it, a pairing the consumer
// upholds. Stated here so the contract holds whatever shape a boxing ABI
// gives the payload.
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

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

    /// All-zero `Value`. Under word boxing this matches
    /// `mrb_nil_value()` (MRB_Qnil = 0), but callers that need a
    /// guaranteed nil should prefer
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
        Self(Immediates::get().qnil)
    }

    /// Canonical mruby `true`. See `Value::nil`.
    #[inline]
    pub fn true_() -> Self {
        Self(Immediates::get().qtrue)
    }

    /// Canonical mruby `false`. See `Value::nil`.
    #[inline]
    pub fn false_() -> Self {
        Self(Immediates::get().qfalse)
    }

    /// `mrb_int_value(mrb, n)` — construct an mruby Integer from `n`,
    /// via mruby's own boxing-agnostic `MRB_INLINE` constructor
    /// (reached through bindgen's static-fn trampoline, compiled with
    /// the same defines as the linked archive). `sys::mrb_int` follows
    /// the archive's configured width — 64-bit under mruby's 64-bit
    /// platform default, 32-bit under `MRB_INT32` or on wasm32.
    #[inline]
    pub fn from_int(mrb: &Mrb, n: sys::mrb_int) -> Self {
        // SAFETY: `mrb` is alive by the `&Mrb` borrow.
        Self(unsafe { sys::mrb_int_value(mrb.as_ptr(), n) })
    }

    /// `mrb_float_value(mrb, f)` — construct an mruby Float from `f`,
    /// via mruby's boxing-agnostic `MRB_INLINE` constructor (same
    /// trampoline route as `Value::from_int`).
    #[inline]
    pub fn from_float(mrb: &Mrb, f: sys::mrb_float) -> Self {
        // SAFETY: `mrb` is alive by the `&Mrb` borrow.
        Self(unsafe { sys::mrb_float_value(mrb.as_ptr(), f) })
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
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_float_to_integer` raises
            // `TypeError` on a non-Float receiver and `RangeError` on an
            // infinite or NaN float — both caught by `protect` into `Err`
            // — and otherwise returns an Integer value.
            Value(unsafe { sys::mrb_float_to_integer(mrb.as_ptr(), self.0) })
        })
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
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self` and
            // `other` originate from the same VM. `mrb_num_add` raises
            // `TypeError` on a non-numeric operand and `RangeError` on an
            // integer result past the configured width — both caught by
            // `protect` into `Err`.
            Value(unsafe { sys::mrb_num_add(mrb.as_ptr(), self.0, other.0) })
        })
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
        mrb.protect(|mrb| {
            // SAFETY: as `Value::add`. `mrb_num_sub` raises `TypeError` on
            // a non-numeric operand and `RangeError` on an integer result
            // past the configured width — both caught by `protect`.
            Value(unsafe { sys::mrb_num_sub(mrb.as_ptr(), self.0, other.0) })
        })
    }

    /// Multiply `self` by `other`, Ruby's `*` on `Integer` and `Float`. The
    /// result type and raises mirror `Value::add`: an Integer when both
    /// operands are integers and the result fits the configured width, a Float
    /// when either is a float; a non-numeric operand raises `TypeError` and an
    /// integer result past the configured width raises `RangeError`, both
    /// caught by `Mrb::protect`. Anchors on mruby's own `mrb_num_mul`.
    #[inline]
    pub fn mul(self, mrb: &Mrb, other: Value) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: as `Value::add`. `mrb_num_mul` raises `TypeError` on
            // a non-numeric operand and `RangeError` on an integer result
            // past the configured width — both caught by `protect`.
            Value(unsafe { sys::mrb_num_mul(mrb.as_ptr(), self.0, other.0) })
        })
    }

    /// Coerce `self` to a string value — `self` unchanged when it is
    /// already a string, otherwise the result of its `to_s`. Runs under
    /// `Mrb::protect`: `Ok` with the string value, or `Err` when `to_s`
    /// does not return a string. Mirrors mruby's `mrb_obj_as_string`.
    #[inline]
    pub fn obj_as_string(self, mrb: &Mrb) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_obj_as_string` may run
            // `to_s` and raise — caught by `protect` into `Err`.
            Value(unsafe { sys::mrb_obj_as_string(mrb.as_ptr(), self.0) })
        })
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
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_ensure_int_type` raises
            // `TypeError` on a non-numeric value and `RangeError` on an
            // infinite or NaN Float — both caught by `protect` into
            // `Err` — and otherwise returns an Integer value.
            Value(unsafe { sys::mrb_ensure_int_type(mrb.as_ptr(), self.0) })
        })
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
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_ensure_float_type` raises
            // `TypeError` on a non-numeric value — caught by `protect`
            // into `Err` — and otherwise returns a Float value.
            Value(unsafe { sys::mrb_ensure_float_type(mrb.as_ptr(), self.0) })
        })
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

    /// `obj.dup` — a shallow copy of `self`: its instance variables are
    /// copied (not the objects they reference), the copy is unfrozen and
    /// carries no singleton class, and the class's `initialize_copy`
    /// runs on it. An immediate returns itself. Runs under `Mrb::protect`:
    /// `Ok` with the copy, or `Err` when `initialize_copy` raises.
    /// Mirrors mruby's `mrb_obj_dup`.
    #[inline]
    pub fn obj_dup(self, mrb: &Mrb) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_obj_dup` runs
            // `initialize_copy` and may raise — caught by `protect`.
            Value(unsafe { sys::mrb_obj_dup(mrb.as_ptr(), self.0) })
        })
    }

    /// `obj.clone` — like `dup` but also copies the singleton class and
    /// the frozen state, the deeper of the two duplications; the class's
    /// `initialize_copy` runs on the copy. An immediate returns itself.
    /// Runs under `Mrb::protect`: `Ok` with the copy, or `Err` when
    /// `initialize_copy` raises. Mirrors mruby's `mrb_obj_clone`.
    #[inline]
    pub fn obj_clone(self, mrb: &Mrb) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame; `self`
            // originates from the same VM. `mrb_obj_clone` runs
            // `initialize_copy` and may raise — caught by `protect`.
            Value(unsafe { sys::mrb_obj_clone(mrb.as_ptr(), self.0) })
        })
    }

    /// `mrb_obj_classname(mrb, self)` — the Ruby class name of `self`
    /// as an owned `String`, or `""` when mruby returns NULL. mruby
    /// builds the name into a GC-managed temporary, so the bytes are
    /// copied out at once rather than borrowed.
    #[inline]
    pub fn classname(self, mrb: &Mrb) -> String {
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
        let Ok(s_val) = self.funcall(mrb, c"to_s", &[]) else {
            return String::new();
        };
        s_val.string_lossy(mrb)
    }

    /// Read a String-tagged value into an owned UTF-8 `String`,
    /// collapsing a non-String tag or non-UTF-8 bytes to an empty
    /// string — the shared render tail of `to_string`, `inspect`, and
    /// `Error::backtrace`.
    /// The String tag, not the classname, decides: a String subclass
    /// instance reads its bytes the same way a plain String does, the
    /// rule the `FromValue` downcasts follow.
    #[inline]
    pub(crate) fn string_lossy(self, mrb: &Mrb) -> String {
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

    /// `mrb_any_to_s(mrb, self)` — the value's default `to_s` render as a
    /// new `RString`: `#<ClassName>` for an immediate, `#<ClassName:0x...>`
    /// for a heap object. Built from the class name without dispatching the
    /// value's own `to_s`, so it is the render `obj_as_string` falls back to
    /// and runs no user Ruby — total, returning the string directly.
    #[inline]
    pub fn any_to_s(self, mrb: &Mrb) -> crate::RString {
        // SAFETY: `mrb` is alive; `self` originates from the same VM.
        // `mrb_any_to_s` reads the class name and object id only, so it
        // returns a String-tagged value without dispatching user Ruby —
        // the unchecked wrap accepts it.
        let v = Value::from_raw(unsafe { sys::mrb_any_to_s(mrb.as_ptr(), self.0) });
        unsafe { RString::from_value_unchecked(v) }
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
        // SAFETY: forwarded from caller.
        unsafe { sys::mrb_class_ptr_func(self.0) }
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
        let sym = name.into_sym(mrb);
        self.funcall_argv(mrb, sym, args)
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
                    sys::mrb_int::try_from(args.len()).unwrap_or(sys::mrb_int::MAX),
                    argv,
                )
            })
        })
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
                    sys::mrb_int::try_from(args.len()).unwrap_or(sys::mrb_int::MAX),
                    argv,
                    block_raw,
                )
            })
        })
    }

    /// TRUE when `self` is `nil`. Pure tag predicate via mruby's
    /// `mrb_nil_p(v)`, reached through bindgen's static-fn trampoline
    /// — the `wrapper.h` shim wraps the macro so the C compiler reads
    /// the boxing-config layout the archive was built with.
    #[inline]
    pub fn is_nil(self) -> bool {
        // SAFETY: mrb_nil_p is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_nil_p_func(self.0) }
    }

    /// Ruby truthiness: TRUE for every value except `nil` and `false`.
    /// This is the `if` test, not a type check — routes through mruby's
    /// `mrb_test` shim so the boxing-config layout matches the linked
    /// archive, like `Value::is_nil`. Pair with `FromValue for bool`,
    /// which reads a value through this rule.
    #[inline]
    pub fn to_bool(self) -> bool {
        // SAFETY: mrb_test is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_test_func(self.0) }
    }

    /// TRUE when `self` is exactly Ruby `true`. See `Value::is_nil` for
    /// the boxing-config routing.
    #[inline]
    pub fn is_true(self) -> bool {
        // SAFETY: mrb_true_p is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_true_p_func(self.0) }
    }

    /// TRUE when `self` is exactly Ruby `false` — `nil` is excluded.
    /// `nil` and `false` share the `MRB_TT_FALSE` tag under some boxing
    /// modes, so this must route through mruby's `mrb_false_p` shim
    /// rather than a tag test, which would misread `nil`.
    #[inline]
    pub fn is_false(self) -> bool {
        // SAFETY: mrb_false_p is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_false_p_func(self.0) }
    }

    /// TRUE when `self` carries `MRB_TT_INTEGER`. Pure tag predicate
    /// via mruby's `mrb_type` (`MRB_INLINE`), reached through
    /// bindgen's static-fn trampoline. Pair with
    /// `Value::unbox_integer` for the direct-unbox path.
    #[inline]
    pub fn is_integer(self) -> bool {
        // SAFETY: mrb_type is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_INTEGER }
    }

    /// TRUE when `self` carries `MRB_TT_FLOAT`. See `Value::is_integer`.
    /// Pair with `Value::unbox_float`.
    #[inline]
    pub fn is_float(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_FLOAT }
    }

    /// TRUE when `self` carries `MRB_TT_ARRAY`. See `Value::is_integer`.
    /// Pair with `Array::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_array(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_ARRAY }
    }

    /// TRUE when `self` carries `MRB_TT_HASH`. See `Value::is_integer`.
    /// Pair with `Hash::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_hash(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_HASH }
    }

    /// TRUE when `self` carries `MRB_TT_CLASS` — the class tag only;
    /// modules (`MRB_TT_MODULE`) and singleton classes
    /// (`MRB_TT_SCLASS`) are excluded per SPEC's downcast rule. See
    /// `Value::is_integer`. Pair with `Value::as_class_ptr` for the
    /// direct-unbox path.
    #[inline]
    pub fn is_class(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_CLASS }
    }

    /// TRUE when `self` carries `MRB_TT_MODULE` — the module tag only;
    /// classes (`MRB_TT_CLASS`) are excluded, the complement of
    /// `Value::is_class`. See `Value::is_integer`. No typed handle binds
    /// this tag yet, so the predicate stands alone.
    #[inline]
    pub fn is_module(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_MODULE }
    }

    /// TRUE when `self` carries `MRB_TT_PROC`. See `Value::is_integer`.
    /// Pair with `Proc::from_value_unchecked` for the direct-wrap path.
    #[inline]
    pub fn is_proc(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_PROC }
    }

    /// TRUE when `self` carries `MRB_TT_CDATA` — a Rust value wrapped
    /// through the data-carrier seam. See `Value::is_integer`. Pair
    /// with `Value::data_get` for the type-checked extraction path.
    #[inline]
    pub fn is_data(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_CDATA }
    }

    /// TRUE when `self` carries `MRB_TT_STRING`. See `Value::is_integer`.
    /// Pair with `RString::as_bytes` for the byte-borrow path.
    #[inline]
    pub fn is_string(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_STRING }
    }

    /// TRUE when `self` carries `MRB_TT_SYMBOL`. See `Value::is_integer`.
    /// Pair with `Symbol::from_value` for the checked downcast path.
    #[inline]
    pub fn is_symbol(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_SYMBOL }
    }

    /// TRUE when `self` carries `MRB_TT_RANGE`. See `Value::is_integer`.
    /// No typed handle binds this tag yet, so the predicate stands alone.
    #[inline]
    pub fn is_range(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_RANGE }
    }

    /// TRUE when `self` carries `MRB_TT_EXCEPTION` — the exception-object
    /// tag, the type every `raise`d value carries; an arbitrary class that
    /// merely descends from `Exception` is not yet an instance and reads
    /// FALSE. See `Value::is_integer`. No typed handle binds this tag yet,
    /// so the predicate stands alone.
    #[inline]
    pub fn is_exception(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_EXCEPTION }
    }

    /// View `self` as a typed `Break` when it carries mruby's break
    /// tag (`MRB_TT_BREAK`), or `None` for any other tag. A break
    /// surfaces as the value inside the `Err` of a protected
    /// `Proc::call` when the block exits via a non-local `break` or
    /// `return`; classifying that exit is the caller's policy.
    #[inline]
    pub fn as_break(self) -> Option<Break> {
        // SAFETY: mrb_break_p_func is a pure predicate over the
        // value tag and does not touch mrb_state. The tag check
        // establishes the `Break` newtype's invariant.
        unsafe { sys::mrb_break_p_func(self.0) }.then_some(Break(self))
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
        // SAFETY: forwarded from caller.
        unsafe { sys::mrb_integer_func(self.0) }
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
        // SAFETY: forwarded from caller.
        unsafe { sys::mrb_float_func(self.0) }
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
        // SAFETY: forwarded from caller.
        Value(unsafe { sys::mrb_ary_entry(self.0, idx) })
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

    /// `mrb_iv_get(mrb, self, sym)` — return instance variable `sym`
    /// from `self`, or `nil` when unset.
    #[inline]
    pub fn iv_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Value {
        // SAFETY: as `iv_set`.
        Value(unsafe { sys::mrb_iv_get(mrb.as_ptr(), self.0, sym) })
    }

    /// `mrb_iv_defined(mrb, self, sym)` — TRUE when instance variable
    /// `sym` is set on `self`. A receiver that cannot hold instance
    /// variables reads as FALSE rather than raising. The value-level
    /// analogue of the raw-`RObject*` `mrb_obj_iv_defined`, which stays
    /// in `sys`.
    #[inline]
    pub fn iv_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        // SAFETY: as `iv_set`.
        unsafe { sys::mrb_iv_defined(mrb.as_ptr(), self.0, sym) }
    }

    /// `mrb_iv_remove(mrb, self, sym)` — remove instance variable `sym`
    /// from `self`, returning `Some` of its former value. Yields `None`
    /// when the variable is absent or `self` cannot hold instance
    /// variables, distinguishing either case from a variable removed
    /// while holding `nil`. Surfaces an `Err` only when a frozen `self`
    /// can hold instance variables.
    #[inline]
    pub fn iv_remove(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Option<Value>, Error> {
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

    /// `mrb_iv_foreach(mrb, self, …)` — visit each instance variable set
    /// on `self` in iv-table order, handing its name as a typed `Symbol`
    /// and its value to `body`. Returning `ForEach::Stop` ends the
    /// iteration before the remaining variables; `ForEach::Continue`
    /// proceeds. The iteration visits the variables and the values they
    /// held when it began: `body` reassigning, removing, or adding the
    /// receiver's instance variables changes the receiver but never the
    /// visited set, and each visited value holds arena protection as if
    /// created here, staying valid across `body`'s own mutations and
    /// collections. A receiver that holds no instance variables — an
    /// immediate, or one that never had any — is visited zero times. The
    /// iteration dispatches no Ruby and so never raises; magnus binds no
    /// ivar foreach, so this anchors on mruby's own `mrb_iv_foreach`.
    ///
    /// A panic in `body` ends the iteration and propagates; `body` runs
    /// after the C walk has finished, so the panic never crosses
    /// mruby's frames.
    #[inline]
    pub fn each_iv<F>(self, mrb: &Mrb, body: F)
    where
        F: FnMut(crate::Symbol, Value) -> crate::ForEach,
    {
        // Snapshot the (name, value) pairs before any caller code
        // runs: the C foreach walks the live iv table, which `body`
        // re-entering the VM could free and reallocate mid-walk, so
        // `body` only ever runs against this collected copy. Each
        // value is arena-protected as it is collected — the receiver
        // stops referencing a value `body` removes, and the snapshot
        // must outlive any collection `body` triggers.
        unsafe extern "C" fn collect(
            mrb: *mut sys::mrb_state,
            name: sys::mrb_sym,
            val: sys::mrb_value,
            data: *mut core::ffi::c_void,
        ) -> core::ffi::c_int {
            // SAFETY: `data` is the `&mut Vec<…>` handed to
            // `mrb_iv_foreach` below, borrowed for the duration of
            // the walk on this same thread; `mrb` is the live state
            // driving the walk, and protecting into the arena leaves
            // the iv table untouched.
            let pairs: &mut Vec<(crate::Symbol, Value)> =
                unsafe { &mut *(data as *mut Vec<(crate::Symbol, Value)>) };
            unsafe { sys::mrb_gc_protect(mrb, val) };
            pairs.push((crate::Symbol::from_sym(name), Value::from_raw(val)));
            0
        }

        let mut pairs: Vec<(crate::Symbol, Value)> = Vec::new();
        // SAFETY: `mrb` is alive; `self` originates from the same VM.
        // `mrb_iv_foreach` guards a receiver that cannot hold instance
        // variables and returns without calling back. `collect`
        // upholds the `mrb_iv_foreach_func` ABI and runs no caller
        // code; `data` points to `pairs` on this frame, which outlives
        // the call. bindgen wraps the function-typedef parameter in
        // `Option`, so the collector is passed via `Some`.
        unsafe {
            sys::mrb_iv_foreach(
                mrb.as_ptr(),
                self.0,
                Some(collect),
                &mut pairs as *mut Vec<(crate::Symbol, Value)> as *mut core::ffi::c_void,
            );
        }
        let mut body = body;
        for (name, val) in pairs {
            if let crate::ForEach::Stop = body(name, val) {
                break;
            }
        }
    }

    /// TRUE when `self` is a class, module, or singleton class — the
    /// receiver family that owns constants and class variables, the
    /// same set mruby's own constant accessors accept.
    fn is_class_or_module(self) -> bool {
        // SAFETY: as `is_integer`.
        matches!(
            unsafe { sys::mrb_type(self.0) },
            sys::MRB_TT_CLASS | sys::MRB_TT_MODULE | sys::MRB_TT_SCLASS
        )
    }

    /// The `TypeError` the class-variable accessors surface for a
    /// receiver that is not a class or module — the rejection the
    /// constant accessors inherit from mruby's own receiver check.
    fn not_class_or_module_error(self, mrb: &Mrb) -> Error {
        let msg = format!("{} is not a class/module", self.classname(mrb));
        Error::Exception(crate::method::core_exception(mrb, c"TypeError", &msg))
    }

    /// `mrb_const_defined(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined on `self` (the module or class value), walking the
    /// ancestry. Answers false when `self` is not a class or module.
    #[inline]
    pub fn const_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        if !self.is_class_or_module() {
            return false;
        }
        // SAFETY: as `iv_set`, with the class-or-module receiver
        // `mrb_const_defined` dereferences unchecked established
        // by the guard above.
        unsafe { sys::mrb_const_defined(mrb.as_ptr(), self.0, sym) }
    }

    /// `mrb_const_defined_at(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined directly on `self` alone, never one inherited from an
    /// ancestor; contrast `const_defined`, which walks the ancestry.
    /// Answers false when `self` is not a class or module.
    #[inline]
    pub fn const_defined_at(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        if !self.is_class_or_module() {
            return false;
        }
        // SAFETY: as `iv_set`, with the class-or-module receiver
        // `mrb_const_defined_at` dereferences unchecked established
        // by the guard above.
        unsafe { sys::mrb_const_defined_at(mrb.as_ptr(), self.0, sym) }
    }

    /// `mrb_const_get(mrb, self, sym)` — fetch the constant value at
    /// `sym` from `self`. Surfaces an `Err` when `sym` resolves to no
    /// constant or its `const_missing` hook raises.
    #[inline]
    pub fn const_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Value, Error> {
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM. `mrb_const_get`
            // raises `NameError` for an undefined constant and runs
            // a `const_missing` hook that may raise — both caught
            // by `protect`.
            Value(unsafe { sys::mrb_const_get(mrb.as_ptr(), self.0, sym) })
        })
    }

    /// `mrb_const_set(mrb, self, sym, val)` — assign constant `sym` on
    /// `self` (the module or class value) to `val`. Surfaces an `Err`
    /// when `self` is not a class or module, when `self` is frozen, or
    /// when the `const_added` hook raises. The value-level write
    /// complementing `const_get`.
    #[inline]
    pub fn const_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) -> Result<(), Error> {
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

    /// `mrb_const_remove(mrb, self, sym)` — remove constant `sym` from
    /// `self` (the module or class value), discarding its former value.
    /// An absent constant is a no-op; surfaces an `Err` when `self` is
    /// not a class or module, or when it is frozen.
    #[inline]
    pub fn const_remove(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<(), Error> {
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

    /// `mrb_cv_get(mrb, self, sym)` — read class variable `sym` from
    /// `self` (the module or class value), walking the ancestry.
    /// Surfaces an `Err` when `self` is not a class or module, or when
    /// `sym` resolves to no class variable.
    #[inline]
    pub fn cv_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Result<Value, Error> {
        if !self.is_class_or_module() {
            return Err(self.not_class_or_module_error(mrb));
        }
        mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `self` originates from the same VM. `mrb_cv_get`
            // raises `NameError` for an undefined class variable —
            // caught by `protect`.
            Value(unsafe { sys::mrb_cv_get(mrb.as_ptr(), self.0, sym) })
        })
    }

    /// `mrb_cv_set(mrb, self, sym, val)` — assign class variable `sym`
    /// on `self` (the module or class value) to `val`. Surfaces an
    /// `Err` when `self` is not a class or module, or is frozen. The
    /// value-level write complementing `cv_get`; `mrb_mod_cv_set` (the
    /// raw-`RClass*` form) stays in `sys`.
    #[inline]
    pub fn cv_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) -> Result<(), Error> {
        if !self.is_class_or_module() {
            return Err(self.not_class_or_module_error(mrb));
        }
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

    /// `mrb_cv_defined(mrb, self, sym)` — TRUE when class variable `sym`
    /// is defined on `self` (the module or class value) or any ancestor.
    /// Answers false when `self` is not a class or module. The
    /// value-level analogue of the raw-`RClass*` `mrb_mod_cv_defined`,
    /// which stays in `sys`.
    #[inline]
    pub fn cv_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        if !self.is_class_or_module() {
            return false;
        }
        // SAFETY: as `iv_set`, with the class-or-module receiver
        // `mrb_cv_defined` dereferences unchecked established by
        // the guard above.
        unsafe { sys::mrb_cv_defined(mrb.as_ptr(), self.0, sym) }
    }

    /// `mrb_respond_to(mrb, self, mid)` — TRUE when `self` answers to
    /// the method named by `mid`.
    #[inline]
    pub fn respond_to(self, mrb: &Mrb, mid: sys::mrb_sym) -> bool {
        // SAFETY: as `iv_set`.
        unsafe { sys::mrb_respond_to(mrb.as_ptr(), self.0, mid) }
    }

    /// `mrb_obj_class(mrb, self)` — the class `self` belongs to, Ruby's
    /// `Object#class`. Every value has a class, so this never fails.
    #[inline]
    pub fn class(self, mrb: &Mrb) -> RClass {
        // SAFETY: `mrb` is alive; `self` shares the VM. `mrb_obj_class`
        // returns the receiver's class pointer, never null.
        RClass::from_raw(unsafe { sys::mrb_obj_class(mrb.as_ptr(), self.0) })
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

    /// `mrb_obj_is_kind_of(mrb, self, class)` — whether `self` is an
    /// instance of `class` or any of its subclasses, Ruby's `is_a?`. A
    /// pure ancestry walk that dispatches nothing, so it never raises.
    #[inline]
    pub fn is_kind_of(self, mrb: &Mrb, class: RClass) -> bool {
        // SAFETY: `mrb` is alive; `self` and `class` share the VM.
        // `mrb_obj_is_kind_of` only walks the class hierarchy.
        unsafe { sys::mrb_obj_is_kind_of(mrb.as_ptr(), self.0, class.as_raw()) }
    }

    /// `mrb_obj_is_instance_of(mrb, self, class)` — whether `self` is a
    /// direct instance of `class`, Ruby's `instance_of?`. A pure class
    /// compare that dispatches nothing, so it never raises.
    #[inline]
    pub fn is_instance_of(self, mrb: &Mrb, class: RClass) -> bool {
        // SAFETY: as `is_kind_of`; `mrb_obj_is_instance_of` only reads
        // the receiver's class.
        unsafe {
            sys::mrb_obj_is_instance_of(mrb.as_ptr(), self.0, class.as_raw() as *const sys::RClass)
        }
    }

    /// `mrb_obj_freeze(mrb, self)` — freeze `self` in place and return
    /// it, Ruby's `Object#freeze`. Freezing is idempotent and never
    /// raises.
    #[inline]
    pub fn freeze(self, mrb: &Mrb) -> Value {
        // SAFETY: `mrb` is alive; `self` shares the VM. `mrb_obj_freeze`
        // sets the frozen flag and returns the receiver.
        Value::from_raw(unsafe { sys::mrb_obj_freeze(mrb.as_ptr(), self.0) })
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

    /// `mrb_obj_equal(mrb, self, other)` — TRUE when `self` and `other`
    /// are the same object, Ruby's `equal?`. A pure identity compare:
    /// it dispatches nothing, so it never raises and yields a `bool`.
    #[inline]
    pub fn obj_equal(self, mrb: &Mrb, other: Value) -> bool {
        // SAFETY: `mrb` is alive; `self` and `other` share the VM by
        // the single-VM contract. `mrb_obj_equal` only inspects the
        // two values' identity.
        unsafe { sys::mrb_obj_equal(mrb.as_ptr(), self.0, other.0) }
    }

    /// `mrb_obj_id(self)` — a unique integer identifier for `self`,
    /// Ruby's `object_id`. Reads the value's identity from the boxed
    /// word alone, so it takes no `Mrb`, dispatches nothing, and never
    /// raises.
    #[inline]
    pub fn object_id(self) -> sys::mrb_int {
        // SAFETY: `mrb_obj_id` reads only `self`'s boxed word for its
        // identity and does not touch `mrb_state`.
        unsafe { sys::mrb_obj_id(self.0) }
    }

    /// `mrb_equal(mrb, self, other)` — Ruby `==` equality. May run a
    /// user-defined `==`, so it runs under the same protection as
    /// `Mrb::protect`: `Ok(bool)` for the comparison, or `Err` when the
    /// dispatched method raises.
    #[inline]
    pub fn equal(self, mrb: &Mrb, other: Value) -> Result<bool, Error> {
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

    /// `mrb_eql(mrb, self, other)` — Ruby `eql?`, the stricter equality
    /// `Hash` keys use. May run a user-defined `eql?`, so like `equal`
    /// it runs under protection: `Ok(bool)` or `Err` on a raise.
    #[inline]
    pub fn eql(self, mrb: &Mrb, other: Value) -> Result<bool, Error> {
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

    /// `mrb_as_float(mrb, self)` — convert `self` to a Rust float across
    /// the numeric types: a Float reads directly and an Integer widens to
    /// a float. A non-numeric value raises `TypeError`, so like `as_int`
    /// the conversion runs under `Mrb::protect` and dispatches no `to_f`.
    /// Distinct from `f64::from_value`, the exact-tag downcast that never
    /// converts across types and rejects an Integer outright.
    ///
    /// The converted number round-trips through `Value::from_float` and
    /// `unbox_float` at the archive's own float width, so nothing is
    /// lost between them.
    #[inline]
    pub fn as_float(self, mrb: &Mrb) -> Result<sys::mrb_float, Error> {
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
        // SAFETY: `self.0` is break-tagged by the `Value::as_break`
        // gate that is this newtype's only constructor.
        Value::from_raw(unsafe { sys::mrb_break_value_func(self.0.as_raw()) })
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

    #[test]
    fn typed_handles_cross_threads() {
        // Every typed handle is a newtype over `Value`, so the markers
        // stated on `Value` are what carry the whole family. A handle
        // that grew a field of its own leaves the family here.
        fn crosses<T: Send + Sync>() {}
        crosses::<crate::Value>();
        crosses::<crate::Break>();
        crosses::<crate::Array>();
        crosses::<crate::Hash>();
        crosses::<crate::Proc>();
        crosses::<crate::Range>();
        crosses::<crate::RString>();
        crosses::<crate::Symbol>();
    }
}
