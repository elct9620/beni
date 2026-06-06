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

use crate::Mrb;

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

    /// `mrb_obj_as_string(mrb, self)` — coerce `self` to a String
    /// value via mruby's public coercion helper. Returns `self`
    /// unchanged when it is already a String; otherwise calls `to_s`
    /// and returns the result. May raise `TypeError` when the
    /// receiver's `to_s` does not return a String — only safe to
    /// call from contexts that can absorb a longjmp (C bridges,
    /// `mrb_protect_error` bodies).
    #[inline]
    pub fn obj_as_string(self, mrb: &Mrb) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the borrow; `self` originates
            // from the same VM.
            Value::from_raw(unsafe { sys::mrb_obj_as_string(mrb.as_ptr(), self.0) })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = mrb;
            crate::not_linked()
        }
    }

    /// Borrow the raw bytes of a String-tagged `self`. Routes through
    /// the `mrb_rstring_ptr` / `mrb_rstring_len` static-inline
    /// wrappers in `wrapper.h`, which expand the `RSTRING_PTR(s)` /
    /// `RSTRING_LEN(s)` macros inside the C compiler so the
    /// embed-vs-heap branch comes from mruby's own header rather
    /// than a Rust-side mirror.
    ///
    /// The returned slice points at storage owned by the mruby VM;
    /// the `&Mrb` borrow keeps the state alive for the slice's
    /// lifetime, but does not block GC or string mutation. Callers
    /// must consume the slice before the next mruby call that could
    /// touch this string.
    ///
    /// # Safety
    ///
    /// `self` must be a String-tagged `Value`. Caller must not
    /// invoke another mruby API that could free or move the
    /// string's backing buffer before consuming the slice.
    #[inline]
    pub unsafe fn as_bytes(self, _mrb: &Mrb) -> &[u8] {
        #[cfg(mruby_linked)]
        {
            // SAFETY: forwarded from caller. The wrapper-h inline
            // helpers expand the RSTRING_PTR / RSTRING_LEN macros
            // against mruby's own headers.
            let ptr = unsafe { sys::mrb_rstring_ptr(self.0) } as *const u8;
            let len = unsafe { sys::mrb_rstring_len(self.0) } as usize;
            // SAFETY: ptr / len pair describes a buffer owned by mruby
            // and alive while the borrowed `&Mrb` outlives this slice.
            unsafe { core::slice::from_raw_parts(ptr, len) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
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
    /// Bytes are read through `as_bytes` (RSTRING_PTR / RSTRING_LEN),
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
    /// returns a non-String, the failure is **swallowed**: any pending
    /// `mrb->exc` is cleared and an empty `String` is returned, so the
    /// leaked exception does not corrupt subsequent mruby calls in the
    /// same C bridge.
    #[inline]
    pub fn to_string(self, mrb: &Mrb) -> String {
        #[cfg(mruby_linked)]
        {
            let s_val = self.call(mrb, c"to_s", &[]);
            if !mrb.pending_exc().is_nil() {
                mrb.clear_exc();
                return String::new();
            }
            if s_val.classname(mrb) != "String" {
                return String::new();
            }
            // SAFETY: the classname gate confirms `s_val` is String-tagged;
            // the bytes are copied before any further mruby call.
            let bytes = unsafe { s_val.as_bytes(mrb) };
            core::str::from_utf8(bytes).unwrap_or("").to_string()
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

    /// Invoke `self.<method>(args...)` via the non-variadic
    /// `mrb_funcall_argv`. The method name is interned through
    /// `Mrb::intern_cstr`; use `Value::call_argv` directly when
    /// the caller already holds an interned `sys::mrb_sym` (e.g. a
    /// dispatch site that cached the sym across a `respond_to?`
    /// gate).
    #[inline]
    pub fn call(self, mrb: &Mrb, name: &core::ffi::CStr, args: &[Value]) -> Value {
        #[cfg(mruby_linked)]
        {
            let sym = mrb.intern_cstr(name);
            self.call_argv(mrb, sym, args)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = (mrb, name, args);
            crate::not_linked()
        }
    }

    /// `mrb_funcall_argv(mrb, self, sym, argc, argv)` — invoke the
    /// method already interned as `sym`. Counterpart to `Value::call`
    /// for sites that pre-intern (typically because the same symbol is
    /// queried via `respond_to?` first).
    ///
    /// `args` is `&[Value]`; `Value` is `#[repr(transparent)]` over
    /// `mrb_value`, so the slice layout matches mruby's `mrb_value`
    /// argv exactly — the pointer cast on the way through is a no-op
    /// at codegen level.
    #[inline]
    pub fn call_argv(self, mrb: &Mrb, sym: sys::mrb_sym, args: &[Value]) -> Value {
        #[cfg(mruby_linked)]
        {
            let argv = args.as_ptr() as *const sys::mrb_value;
            // SAFETY: `mrb` is alive by the borrow; `self` and every
            // `args` entry originate from the same VM by the single-VM
            // contract; `sym` was interned against the same VM (caller
            // contract).
            Value(unsafe {
                sys::mrb_funcall_argv(mrb.as_ptr(), self.0, sym, args.len() as sys::mrb_int, argv)
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
    // documentation one-to-one. The `&Mrb` borrow upholds liveness,
    // and `self` provides the receiver — together the methods are
    // safe Rust.
    // ----------------------------------------------------------------

    /// `mrb_iv_set(mrb, self, sym, val)` — assign instance variable
    /// `sym` on `self` to `val`. `self` must be an object value
    /// produced by `mrb`.
    #[inline]
    pub fn iv_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `mrb` is alive by the borrow; `self` and `val`
            // originate from the same VM by the single-VM contract.
            unsafe { sys::mrb_iv_set(mrb.as_ptr(), self.0, sym, val.0) };
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
    /// `sym` from `self`. Sets `mrb->exc` if the constant is
    /// undefined; callers should gate with `Value::const_defined`.
    #[inline]
    pub fn const_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: as `iv_set`.
            Value(unsafe { sys::mrb_const_get(mrb.as_ptr(), self.0, sym) })
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
