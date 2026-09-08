//! `mrb_get_args` shape-typed dispatch on `Mrb`.
//!
//! mruby's `mrb_get_args` is a variadic C function whose format string
//! drives heterogeneous out-parameters at runtime. Rust cannot express
//! that signature directly: a single Rust function cannot vary its
//! return type with a runtime format string, and `extern "C"` variadics
//! force every call site into hand-counted `unsafe` plumbing.
//!
//! The trade is to lift the format string to the *type* level. Each
//! format becomes a zero-sized marker type implementing `Format`;
//! `Mrb::get_args` is the single safe entry point that
//! monomorphises the FFI call against `F::FMT` and returns the typed
//! tuple from `F::Output`.
//!
//!   - `format::O`          — `"o"`   → single positional
//!   - `format::Rest`       — `"*"`   → rest array borrowed from the
//!     call frame
//!   - `format::NRest`      — `"n*"`  → symbol + rest array
//!   - `format::NRestBlock` — `"n*&"` → symbol + rest array + block
//!     slot
//!   - `format::Io`         — `"io"`  → integer + object
//!   - `format::S`          — `"S"`   → single String argument
//!   - `format::Str`        — `"s"`   → String as a borrowed byte slice
//!   - `format::RestBlock`  — `"*&"`  → rest array + block slot
//!   - `format::Kw`         — `":"`   → keyword arguments as a Hash
//!     bucket, kept apart from the positionals
//!   - `format::NRestKwBlock` — `"n*:&"` → symbol + rest array + keyword
//!     Hash bucket + block slot
//!
//! Rest-form variants hand back a slice tied to `&self` — the borrow
//! the bridge body holds for the whole call. The slice stays valid
//! across a VM re-entry (a funcall or an allocation) the body performs
//! while holding it, because mruby projects the `"*"` rest slot through
//! a GC-arena-rooted array rather than the live value stack; a body
//! that re-enters with rest arguments in hand needs no copy of its own.
//! mruby may set the rest pointer to NULL when the rest count is zero —
//! `slice_from_argv` folds that into an empty `&[Value]` so callers do
//! not have to gate on NULL.
//!
//! ## Why a trait rather than per-method wrappers
//!
//! The previous shape was four inherent methods on `Mrb` — one per
//! format string. That worked for a closed set, but every new format
//! widened the `Mrb` surface and duplicated the variadic FFI dance.
//! The trait pattern flips the axis: format identity moves to a ZST,
//! the dispatch surface collapses to a single `get_args::<F>()`, and
//! adding a fifth format means adding a struct + impl — not editing
//! `impl Mrb`. The same pattern is the right template for any other
//! capability cluster that currently lives as fan-of-methods on
//! `Mrb` (`Define`, `Build`, etc.) once a similar combinatorial
//! pressure shows up.
//!
//! ## Extending with a new format
//!
//! Add a marker ZST under `format` and implement `Format`:
//!
//! ```ignore
//! use beni::{Format, Mrb, Value};
//!
//! pub struct Bool; // a not-yet-implemented `"b"` boolean reader
//! impl Format for Bool {
//!     type Output<'a> = Value;
//!     const FMT: &'static core::ffi::CStr = c"b";
//!     fn read(mrb: &Mrb) -> Self::Output<'_> {
//!         // mrb_get_args(mrb, "b", &out) — see `format::O` for the pattern
//!         # unimplemented!()
//!     }
//! }
//! ```

use crate::{Mrb, Value};
use beni_sys as sys;

/// Type-level marker for a single `mrb_get_args` format string.
///
/// Implementors are zero-sized structs (see `format`) whose
/// `Format::FMT` supplies the mruby format and whose
/// `Format::Output` names the typed return shape. The GAT lifetime
/// `'a` carries the borrow from the call-frame argv slot for
/// rest-form formats; immediate formats leave it unused.
///
/// New implementors should monomorphise the `mrb_get_args` call inside
/// `Format::read` against `Format::FMT` — see `format::O` for the
/// minimal pattern.
pub trait Format {
    /// Typed shape returned by `Format::read`. The `'a` lifetime is
    /// the borrow on the call-frame argv slot for rest-form formats;
    /// immediate formats leave it unused.
    type Output<'a>;

    /// mruby format string (e.g. `c"o"`, `c"n*"`). Static-lifetime
    /// `&CStr` so the format byte sequence is interned at compile
    /// time alongside the impl.
    const FMT: &'static core::ffi::CStr;

    /// Read the call-frame argv against `Self::FMT` and project it
    /// into `Format::Output`. The body issues exactly one
    /// `mrb_get_args` call with the per-format out-parameter shape.
    fn read(mrb: &Mrb) -> Self::Output<'_>;
}

impl Mrb {
    /// Read the call-frame argv using a `Format` marker. The
    /// monomorphised call expands to a single `mrb_get_args` against
    /// `F::FMT` and returns the typed tuple from `F::Output`.
    ///
    /// ```ignore
    /// use beni::format::{Io, Rest};
    /// let (fd, mode_val) = mrb.get_args::<Io>();
    /// let argv = mrb.get_args::<Rest>();
    /// ```
    #[inline]
    pub fn get_args<F: Format>(&self) -> F::Output<'_> {
        F::read(self)
    }

    /// Read the single required argument from the call frame. Raises
    /// `ArgumentError` to the Ruby caller unless exactly one positional
    /// argument is present — the strict counterpart to a `format::O`
    /// read, which takes the first slot without checking the count.
    ///
    /// Callable only from a `-1` method body, the frame mruby raises
    /// out of; the long-jump runs no Rust drops, so the caller must
    /// hold no live value needing `Drop`.
    #[inline]
    pub fn arg1(&self) -> Value {
        // SAFETY: `self` is alive by the `&self` borrow. The raise
        // on a wrong argument count long-jumps to the Ruby caller,
        // which the `-1` bridge frame is the contract for.
        Value::from_raw(unsafe { sys::mrb_get_arg1(self.as_ptr()) })
    }

    /// Whether the current call was passed a block. A plain boolean
    /// question about the current call — `true` when a block was given,
    /// `false` otherwise. Total: it never raises. Mirrors magnus's
    /// `Ruby::block_given_p`.
    #[inline]
    pub fn block_given(&self) -> bool {
        // SAFETY: `self` is alive by the `&self` borrow; the read is
        // total — it inspects the current call and never raises.
        unsafe { sys::mrb_block_given_p(self.as_ptr()) }
    }

    /// Read the number of arguments passed to the call frame, splat
    /// arguments counted as their expanded length. Does not raise.
    #[inline]
    pub fn argc(&self) -> sys::mrb_int {
        // SAFETY: `self` is alive by the `&self` borrow; the read is
        // total — it never raises.
        unsafe { sys::mrb_get_argc(self.as_ptr()) }
    }

    /// Read the call frame's positional arguments as a borrowed slice,
    /// the companion to `Mrb::argc`. The slice holds exactly `argc`
    /// values and views the live call frame directly: it must not be
    /// held across a VM re-entry, since a funcall or an allocation can
    /// relocate the value stack and dangle it. To keep positional
    /// arguments across a re-entry, read them through a rest format
    /// (`get_args::<format::Rest>`), whose slice is re-entry-stable.
    /// Splat arguments appear expanded, as the count read sees them. An
    /// empty argument list yields an empty slice. Total: it never raises.
    #[inline]
    pub fn argv(&self) -> &[Value] {
        // SAFETY: `self` is alive by the `&self` borrow. `mrb_get_argv`
        // returns a pointer to `mrb_get_argc` consecutive `mrb_value`s
        // in the current call frame, valid for its duration; both reads
        // derive their length and pointer from the same callinfo so they
        // agree. `slice_from_argv` folds the `argc == 0` case into an
        // empty slice without forming one from the pointer.
        let argv = unsafe { sys::mrb_get_argv(self.as_ptr()) };
        let argc = unsafe { sys::mrb_get_argc(self.as_ptr()) };
        slice_from_argv(argv, argc)
    }
}

/// Zero-sized marker types implementing `Format`. Each marker maps
/// one mruby format string to a typed Rust return.
pub mod format {
    use super::capture_all_kwargs;
    use super::slice_from_argv;
    use super::sys;
    use super::{Format, Mrb, Value};

    /// `mrb_get_args(mrb, "o", &val)` — read a single positional
    /// argument as a `Value`.
    pub struct O;
    impl Format for O {
        type Output<'a> = Value;
        const FMT: &'static core::ffi::CStr = c"o";

        fn read(mrb: &Mrb) -> Value {
            let mut raw = sys::mrb_value::zeroed();
            // SAFETY: `mrb` is alive by the `&Mrb` borrow; `&mut raw`
            // is a valid `*mut mrb_value`; the `"o"` format writes
            // exactly one cell.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut raw as *mut sys::mrb_value,
                );
            }
            Value::from_raw(raw)
        }
    }

    /// `mrb_get_args(mrb, "*", &argv, &argc)` — read the rest array as
    /// a borrowed, re-entry-stable slice (see the module docs on
    /// rest-form borrows).
    pub struct Rest;
    impl Format for Rest {
        type Output<'a> = &'a [Value];
        const FMT: &'static core::ffi::CStr = c"*";

        fn read(mrb: &Mrb) -> &[Value] {
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            // SAFETY: as `O::read`; the `"*"` format writes the argv
            // pointer + length pair.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                );
            }
            slice_from_argv(argv, argc)
        }
    }

    /// `mrb_get_args(mrb, "n*", &sym, &argv, &argc)` — read a leading
    /// symbol followed by a rest array.
    pub struct NRest;
    impl Format for NRest {
        type Output<'a> = (sys::mrb_sym, &'a [Value]);
        const FMT: &'static core::ffi::CStr = c"n*";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value]) {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            // SAFETY: as `O::read`.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                );
            }
            (sym, slice_from_argv(argv, argc))
        }
    }

    /// `mrb_get_args(mrb, "n*&", &sym, &argv, &argc, &block)` — read a
    /// leading symbol, then a rest array, then the block slot from the
    /// call frame. The `&` specifier produces a value copy of the block
    /// `mrb_value` without invoking `mrb_proc_copy`, so the captured
    /// block stays non-orphan
    /// (`vendor/mruby/src/class.c:1593-1604`). When the caller supplied
    /// no block the slot decodes as `mrb_nil`.
    pub struct NRestBlock;
    impl Format for NRestBlock {
        type Output<'a> = (sys::mrb_sym, &'a [Value], Value);
        const FMT: &'static core::ffi::CStr = c"n*&";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value], Value) {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut block_raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`; the `"n*&"` format writes the
            // leading symbol, the argv pointer + length pair, and a
            // single block-slot value.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                    &mut block_raw as *mut sys::mrb_value,
                );
            }
            (sym, slice_from_argv(argv, argc), Value::from_raw(block_raw))
        }
    }

    /// `mrb_get_args(mrb, "io", &n, &val)` — read an integer followed
    /// by an object. The `"i"` specifier writes an `mrb_int`, so the
    /// out-param is typed `sys::mrb_int` (not `c_int`) to match mruby's
    /// own width contract, whatever width the linked archive was
    /// configured with.
    pub struct Io;
    impl Format for Io {
        type Output<'a> = (sys::mrb_int, Value);
        const FMT: &'static core::ffi::CStr = c"io";

        fn read(mrb: &Mrb) -> (sys::mrb_int, Value) {
            let mut n: sys::mrb_int = 0;
            let mut raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut n as *mut sys::mrb_int,
                    &mut raw as *mut sys::mrb_value,
                );
            }
            (n, Value::from_raw(raw))
        }
    }

    /// `mrb_get_args(mrb, "S", &val)` — read a single String argument.
    /// mruby checks the argument is a String (raising `TypeError`
    /// otherwise) before writing, so the result is always a
    /// String-tagged `Value` — the strict counterpart to `O`.
    pub struct S;
    impl Format for S {
        type Output<'a> = Value;
        const FMT: &'static core::ffi::CStr = c"S";

        fn read(mrb: &Mrb) -> Value {
            let mut raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`; the `"S"` format writes exactly
            // one String-checked cell.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut raw as *mut sys::mrb_value,
                );
            }
            Value::from_raw(raw)
        }
    }

    /// `mrb_get_args(mrb, "s", &ptr, &len)` — read a String argument as
    /// a borrowed byte slice pointing at the string's own buffer. mruby
    /// checks the argument is a String before writing. The slice is
    /// valid while that String is unmodified; a body that mutates or
    /// reallocates the argument String while holding the slice
    /// invalidates it. A zero-length string folds to an empty slice.
    pub struct Str;
    impl Format for Str {
        type Output<'a> = &'a [u8];
        const FMT: &'static core::ffi::CStr = c"s";

        fn read(mrb: &Mrb) -> &[u8] {
            let mut ptr: *const core::ffi::c_char = core::ptr::null();
            let mut len: sys::mrb_int = 0;
            // SAFETY: as `O::read`; the `"s"` format writes the
            // string's byte pointer + length pair.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut ptr as *mut *const core::ffi::c_char,
                    &mut len as *mut sys::mrb_int,
                );
            }
            if len > 0 && !ptr.is_null() {
                // SAFETY: mruby owns the string buffer for the
                // duration of the call frame, which outlives this
                // borrow; `len` is its byte length.
                unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) }
            } else {
                &[]
            }
        }
    }

    /// `mrb_get_args(mrb, "*&", &argv, &argc, &block)` — read the rest
    /// array followed by the block slot, with no leading symbol. The
    /// cleaner shape for a block-taking method (`gsub` / `scan` with a
    /// block) than `NRestBlock`, which prepends a symbol. The `&`
    /// specifier copies the block value without `mrb_proc_copy`, so the
    /// captured block stays non-orphan; an absent block decodes as nil.
    pub struct RestBlock;
    impl Format for RestBlock {
        type Output<'a> = (&'a [Value], Value);
        const FMT: &'static core::ffi::CStr = c"*&";

        fn read(mrb: &Mrb) -> (&[Value], Value) {
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut block_raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`; the `"*&"` format writes the
            // argv pointer + length pair and a single block-slot
            // value.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                    &mut block_raw as *mut sys::mrb_value,
                );
            }
            (slice_from_argv(argv, argc), Value::from_raw(block_raw))
        }
    }

    /// `mrb_get_args(mrb, ":", &kwargs)` — read the call's keyword
    /// arguments as a `Hash` bucket kept apart from the positionals.
    /// Capture-all: every keyword pair lands in the returned `Hash`, which
    /// is empty rather than nil when the call passed no keywords, and an
    /// explicit positional `Hash` the caller wrote stays among the
    /// positionals rather than folding into it.
    pub struct Kw;
    impl Format for Kw {
        type Output<'a> = crate::Hash;
        const FMT: &'static core::ffi::CStr = c":";

        fn read(mrb: &Mrb) -> crate::Hash {
            let mut out = sys::mrb_value::zeroed();
            let mut kwargs = capture_all_kwargs(&mut out);
            // SAFETY: as `O::read`; the `":"` format reads the keyword
            // dict through the `mrb_kwargs` input struct. Capture-all
            // sends every pair to `rest`, which mruby fills with an
            // empty Hash when none were passed, so `out` is Hash-tagged.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut kwargs as *mut sys::mrb_kwargs,
                );
            }
            // SAFETY: capture-all guarantees `out` is a Hash value.
            unsafe { crate::Hash::from_value_unchecked(Value::from_raw(out)) }
        }
    }

    /// `mrb_get_args(mrb, "n*:&", &sym, &argv, &argc, &kwargs, &block)` —
    /// read a leading symbol, a rest array, the keyword `Hash` bucket, and
    /// the block slot in one read. The shape a signature-blind
    /// `method_missing` proxy needs: the `:` specifier keeps the caller's
    /// keyword arguments in their own `Hash` (empty rather than nil when
    /// none were passed) instead of folding them into the rest, while an
    /// explicit positional `Hash` stays in the rest. An absent block
    /// decodes as nil.
    pub struct NRestKwBlock;
    impl Format for NRestKwBlock {
        type Output<'a> = (sys::mrb_sym, &'a [Value], crate::Hash, Value);
        const FMT: &'static core::ffi::CStr = c"n*:&";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value], crate::Hash, Value) {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut out = sys::mrb_value::zeroed();
            let mut kwargs = capture_all_kwargs(&mut out);
            let mut block_raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`; the `"n*:&"` format writes the
            // leading symbol, the argv pointer + length pair, the
            // keyword dict through the `mrb_kwargs` input struct, and a
            // single block-slot value.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                    &mut kwargs as *mut sys::mrb_kwargs,
                    &mut block_raw as *mut sys::mrb_value,
                );
            }
            // SAFETY: capture-all guarantees `out` is a Hash value.
            let kw = unsafe { crate::Hash::from_value_unchecked(Value::from_raw(out)) };
            (
                sym,
                slice_from_argv(argv, argc),
                kw,
                Value::from_raw(block_raw),
            )
        }
    }
}

/// Cast a `mrb_get_args` rest-form `(*const mrb_value, mrb_int)` pair
/// into a borrowed `&[Value]`. mruby may set the pointer to NULL when
/// the rest count is zero; reading `len` bytes from NULL would be UB,
/// so the helper folds that into an empty slice.
///
/// The slice's lifetime is bound by the caller's `&self` borrow on
/// `Mrb` (the call frame that produced argv).
#[inline]
fn slice_from_argv<'a>(argv: *const sys::mrb_value, argc: sys::mrb_int) -> &'a [Value] {
    if argc > 0 && !argv.is_null() {
        // SAFETY: Value is `#[repr(transparent)]` over mrb_value;
        // mruby owns the buffer for the duration of the call frame
        // which outlives this borrow.
        unsafe { core::slice::from_raw_parts(argv as *const Value, argc as usize) }
    } else {
        &[]
    }
}

/// Build a capture-all `mrb_kwargs` (no name table) whose keyword dict
/// lands in `*out`. Paired with the `:` specifier, mruby routes every
/// keyword pair to `rest` and fills `*out` with an empty Hash — never nil
/// — when the call passed none, so a caller reads `*out` as a Hash
/// unconditionally (`vendor/mruby/src/class.c:1649`).
#[inline]
fn capture_all_kwargs(out: *mut sys::mrb_value) -> sys::mrb_kwargs {
    sys::mrb_kwargs {
        num: 0,
        required: 0,
        table: core::ptr::null(),
        values: core::ptr::null_mut(),
        rest: out,
    }
}
