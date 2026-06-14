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

    /// `mrb_obj_classname(mrb, self)` — return the Ruby class name of
    /// `self` as a borrowed `&'static str`, or `""` when mruby
    /// returns NULL.
    ///
    /// The returned slice points into mruby's interned class-name
    /// storage, which lives for the duration of the `mrb_state`.
    /// Callers that need to retain the name across a GC point should
    /// `.to_string()` it.
    #[inline]
    pub fn classname(self, mrb: &Mrb) -> &'static str {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the borrow; `self` originates
            // from the same VM by the single-VM contract.
            let ptr = unsafe { sys::mrb_obj_classname(mrb.as_ptr(), self.0) };
            if ptr.is_null() {
                return "";
            }
            // SAFETY: mruby's class-name storage lives for the duration
            // of the `mrb_state`; treating it as `'static` is sound for
            // the lifetime of the VM.
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or("")
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

    /// Invoke `self.<method>(args...)` by name, interning it through
    /// `Mrb::intern_cstr`. The method runs arbitrary Ruby, so the call
    /// runs under `Mrb::protect`: a normal return is the `Ok` value, any
    /// raise is `Err` rather than a long-jump across FFI. Use
    /// `Value::funcall_argv` when the caller already holds an interned
    /// `sys::mrb_sym` (e.g. a dispatch site that cached the sym across a
    /// `respond_to?` gate). Mirrors magnus's `funcall`.
    #[inline]
    pub fn funcall(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        args: &[Value],
    ) -> Result<Value, Error> {
        #[cfg(mruby_linked)]
        {
            let sym = mrb.intern_cstr(name);
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
    // Instance variable / constant accessors. The mruby C API spells
    // these as `mrb_iv_set` / `mrb_iv_get` / `mrb_const_defined` /
    // `mrb_const_get` / `mrb_respond_to`; the inherent methods carry
    // the same names so the call shape mirrors the C-side
    // documentation one-to-one. The reads (`iv_get`, `const_defined`,
    // `respond_to`) dispatch nothing and hand back a bare value; the
    // assigning and fetching operations (`iv_set`, `const_get`) can
    // raise, so they route through `protect` and return a `Result`.
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

    /// `mrb_const_defined(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined on `self` (the module or class value).
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
    use crate::{Ccontext, Error, FromValue, IntoValue, Module, Proc};

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
                assert_eq!(exc.class(&mrb).name(&mrb), Some("TypeError"));
            }
            _ => panic!("a non-String value surfaces a TypeError Err"),
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
        assert_eq!(s.class(&mrb).name(&mrb), Some("String"));

        // is_kind_of holds for the direct class and its ancestors;
        // instance_of only for the direct class.
        assert!(s.is_kind_of(&mrb, string_class));
        assert!(s.is_kind_of(&mrb, object_class));
        assert!(s.is_instance_of(&mrb, string_class));
        assert!(!s.is_instance_of(&mrb, object_class));
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
}
