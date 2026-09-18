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
//! Every read answers a `Result`, the shape magnus's `scan_args` has: a
//! call that does not fit the format — too few or too many positionals,
//! or an argument of the wrong type — comes back as the `Err` carrying
//! the exception mruby raised for it, read under a protect with no panic
//! boundary of its own so the raise crosses plain frames alone. A
//! rest-only format fits every call and always answers `Ok`.
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
//! use beni::{Error, Format, Mrb, Value};
//!
//! pub struct Bool; // a not-yet-implemented `"b"` boolean reader
//! impl Format for Bool {
//!     type Output<'a> = Value;
//!     const FMT: &'static core::ffi::CStr = c"b";
//!     fn read(mrb: &Mrb) -> Result<Self::Output<'_>, Error> {
//!         // read_frame(mrb, |mrb| mrb_get_args(mrb, "b", &out)) — see `format::O`
//!         # unimplemented!()
//!     }
//! }
//! ```

use crate::{Error, Mrb, Value};
use beni_sys as sys;

/// Run a call-frame read under exception protection. `mrb_get_args`
/// raises for a call the read's shape does not accept; protected, that
/// raise comes back as the `Err` carrying mruby's exception, across
/// plain frames alone.
pub(crate) fn read_frame(mrb: &Mrb, read: impl FnOnce(&Mrb)) -> Result<(), Error> {
    mrb.protect(|mrb| {
        read(mrb);
        Value::nil()
    })
    .map(|_| ())
}

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
    /// into `Format::Output`, or answer the `Err` a call that does not
    /// fit the format raises. A format that can fail reads through
    /// `read_frame`; a rest-only format cannot fail and reads directly.
    fn read(mrb: &Mrb) -> Result<Self::Output<'_>, Error>;
}

impl Mrb {
    /// Read the call-frame argv using a `Format` marker, answering the
    /// typed tuple from `F::Output`, or the `Err` carrying the exception
    /// mruby raises when the call does not fit the format. Mirrors
    /// magnus's `scan_args`.
    ///
    /// ```ignore
    /// use beni::format::{Io, Rest};
    /// let (fd, mode_val) = mrb.get_args::<Io>()?;
    /// let argv = mrb.get_args::<Rest>()?;
    /// ```
    #[inline]
    pub fn get_args<F: Format>(&self) -> Result<F::Output<'_>, Error> {
        F::read(self)
    }

    /// Read the single required argument from the call frame: the one
    /// positional, or the keyword hash when the call passed keywords
    /// and no positional. Any other count answers the `Err` carrying
    /// mruby's `ArgumentError`.
    #[inline]
    pub fn arg1(&self) -> Result<Value, Error> {
        // SAFETY: `mrb` is alive inside the protect frame; a wrong
        // argument count raises, which `protect` catches.
        self.protect(|mrb| Value::from_raw_unchecked(unsafe { sys::mrb_get_arg1(mrb.as_ptr()) }))
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
    pub fn argc(&self) -> usize {
        // SAFETY: `self` is alive by the `&self` borrow; the read is
        // total — it never raises. mruby counts arguments from zero.
        (unsafe { sys::mrb_get_argc(self.as_ptr()) }) as usize
    }

    /// Read the call frame's positional arguments as a copy of their
    /// own, the companion to `Mrb::argc`. The copy holds exactly `argc`
    /// values and stays valid whatever the body re-enters: the values
    /// are the frame's, kept alive for the whole call. Splat arguments
    /// appear expanded, as the count read sees them. An empty argument
    /// list yields an empty copy. Total: it never fails.
    #[inline]
    pub fn argv(&self) -> Vec<Value> {
        // SAFETY: the view is copied before anything can re-enter.
        unsafe { self.argv_unchecked() }.to_vec()
    }

    /// Read the call frame's positional arguments as a zero-copy view
    /// of the live frame — `Mrb::argv` without its copy.
    ///
    /// # Safety
    ///
    /// The view must not be held across a VM re-entry: a funcall or an
    /// allocation can grow the value stack, which moves it and leaves
    /// the view dangling.
    #[inline]
    pub unsafe fn argv_unchecked(&self) -> &[Value] {
        // SAFETY: `self` is alive by the `&self` borrow. `mrb_get_argv`
        // returns a pointer to `mrb_get_argc` consecutive `mrb_value`s
        // in the current call frame; both reads derive their length and
        // pointer from the same callinfo so they agree. `slice_from_argv`
        // folds the `argc == 0` case into an empty slice without forming
        // one from the pointer.
        let argv = unsafe { sys::mrb_get_argv(self.as_ptr()) };
        let argc = unsafe { sys::mrb_get_argc(self.as_ptr()) };
        slice_from_argv(argv, argc)
    }
}

/// Zero-sized marker types implementing `Format`. Each marker maps
/// one mruby format string to a typed Rust return.
pub mod format {
    use super::capture_all_kwargs;
    use super::read_frame;
    use super::slice_from_argv;
    use super::sys;
    use super::{Error, Format, Mrb, Value};

    /// `mrb_get_args(mrb, "o", &val)` — read a single positional
    /// argument as a `Value`.
    pub struct O;
    impl Format for O {
        type Output<'a> = Value;
        const FMT: &'static core::ffi::CStr = c"o";

        fn read(mrb: &Mrb) -> Result<Value, Error> {
            let mut raw = sys::mrb_value::zeroed();
            read_frame(mrb, |mrb| {
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
            })?;
            Ok(Value::from_raw_unchecked(raw))
        }
    }

    /// `mrb_get_args(mrb, "*", &argv, &argc)` — read the rest array as
    /// a borrowed, re-entry-stable slice (see the module docs on
    /// rest-form borrows).
    pub struct Rest;
    impl Format for Rest {
        type Output<'a> = &'a [Value];
        const FMT: &'static core::ffi::CStr = c"*";

        fn read(mrb: &Mrb) -> Result<&[Value], Error> {
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            // SAFETY: as `O::read`; the `"*"` format writes the argv
            // pointer + length pair and fits every call, so it never
            // raises. It stays outside a protect frame, whose arena
            // restore would unroot the array the rest is copied into.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                );
            }
            Ok(slice_from_argv(argv, argc))
        }
    }

    /// `mrb_get_args(mrb, "n*", &sym, &argv, &argc)` — read a leading
    /// symbol followed by a rest array.
    ///
    /// The symbol read can raise while the rest copy must stay out of a
    /// protect frame, so the call is first checked under protection with
    /// the rest left uncopied (`*!`), then read for real.
    pub struct NRest;
    impl Format for NRest {
        type Output<'a> = (sys::mrb_sym, &'a [Value]);
        const FMT: &'static core::ffi::CStr = c"n*";

        fn read(mrb: &Mrb) -> Result<(sys::mrb_sym, &[Value]), Error> {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            read_frame(mrb, |mrb| {
                // SAFETY: as below, with the rest left uncopied.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        c"n*!".as_ptr(),
                        &mut sym as *mut sys::mrb_sym,
                        &mut argv as *mut *const sys::mrb_value,
                        &mut argc as *mut sys::mrb_int,
                    );
                }
            })?;
            // SAFETY: as `O::read`; the checked call already fit this
            // shape, so this read does not raise.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                );
            }
            Ok((sym, slice_from_argv(argv, argc)))
        }
    }

    /// `mrb_get_args(mrb, "n*&", &sym, &argv, &argc, &block)` — read a
    /// leading symbol, then a rest array, then the block slot from the
    /// call frame. The `&` specifier produces a value copy of the block
    /// `mrb_value` without invoking `mrb_proc_copy`, so the captured
    /// block stays non-orphan
    /// (`vendor/mruby/src/class.c:1593-1604`). When the caller supplied
    /// no block the slot decodes as `mrb_nil`. Checked, then read, as
    /// `NRest` is.
    pub struct NRestBlock;
    impl Format for NRestBlock {
        type Output<'a> = (sys::mrb_sym, &'a [Value], Value);
        const FMT: &'static core::ffi::CStr = c"n*&";

        fn read(mrb: &Mrb) -> Result<(sys::mrb_sym, &[Value], Value), Error> {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut block_raw = sys::mrb_value::zeroed();
            read_frame(mrb, |mrb| {
                // SAFETY: as below, with the rest left uncopied.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        c"n*!&".as_ptr(),
                        &mut sym as *mut sys::mrb_sym,
                        &mut argv as *mut *const sys::mrb_value,
                        &mut argc as *mut sys::mrb_int,
                        &mut block_raw as *mut sys::mrb_value,
                    );
                }
            })?;
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
            Ok((
                sym,
                slice_from_argv(argv, argc),
                Value::from_raw_unchecked(block_raw),
            ))
        }
    }

    /// `mrb_get_args(mrb, "io", &n, &val)` — read an integer followed
    /// by an object. The `"i"` specifier writes an `mrb_int`, so the
    /// out-param is typed `sys::mrb_int` (not `c_int`) to match mruby's
    /// own width contract; the integer comes back as the `i64` that
    /// holds every configured width.
    pub struct Io;
    impl Format for Io {
        type Output<'a> = (i64, Value);
        const FMT: &'static core::ffi::CStr = c"io";

        fn read(mrb: &Mrb) -> Result<(i64, Value), Error> {
            let mut n: sys::mrb_int = 0;
            let mut raw = sys::mrb_value::zeroed();
            read_frame(mrb, |mrb| {
                // SAFETY: as `O::read`.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        Self::FMT.as_ptr(),
                        &mut n as *mut sys::mrb_int,
                        &mut raw as *mut sys::mrb_value,
                    );
                }
            })?;
            Ok((crate::value::widen(n), Value::from_raw_unchecked(raw)))
        }
    }

    /// `mrb_get_args(mrb, "S", &val)` — read a single String argument.
    /// mruby checks the argument is a String, answering the `TypeError`
    /// as the `Err` otherwise, so the result is always a String-tagged
    /// `Value` — the strict counterpart to `O`.
    pub struct S;
    impl Format for S {
        type Output<'a> = Value;
        const FMT: &'static core::ffi::CStr = c"S";

        fn read(mrb: &Mrb) -> Result<Value, Error> {
            let mut raw = sys::mrb_value::zeroed();
            read_frame(mrb, |mrb| {
                // SAFETY: as `O::read`; the `"S"` format writes exactly
                // one String-checked cell.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        Self::FMT.as_ptr(),
                        &mut raw as *mut sys::mrb_value,
                    );
                }
            })?;
            Ok(Value::from_raw_unchecked(raw))
        }
    }

    /// `mrb_get_args(mrb, "s", &ptr, &len)` — read a String argument's
    /// bytes as a copy of their own, so nothing the body later does to
    /// the String reaches them. mruby checks the argument is a String,
    /// answering the `TypeError` as the `Err` otherwise. A zero-length
    /// string yields an empty copy.
    pub struct Str;
    impl Format for Str {
        type Output<'a> = Vec<u8>;
        const FMT: &'static core::ffi::CStr = c"s";

        fn read(mrb: &Mrb) -> Result<Vec<u8>, Error> {
            let mut ptr: *const core::ffi::c_char = core::ptr::null();
            let mut len: sys::mrb_int = 0;
            read_frame(mrb, |mrb| {
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
            })?;
            if len > 0 && !ptr.is_null() {
                // SAFETY: the String is a frame argument, alive for the
                // call, and nothing has run since the read; the bytes
                // are copied before this returns.
                Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec())
            } else {
                Ok(Vec::new())
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

        fn read(mrb: &Mrb) -> Result<(&[Value], Value), Error> {
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut block_raw = sys::mrb_value::zeroed();
            // SAFETY: as `Rest::read`; the `"*&"` format also writes a
            // single block-slot value and fits every call.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut sys::mrb_int,
                    &mut block_raw as *mut sys::mrb_value,
                );
            }
            Ok((
                slice_from_argv(argv, argc),
                Value::from_raw_unchecked(block_raw),
            ))
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

        fn read(mrb: &Mrb) -> Result<crate::Hash, Error> {
            // The bucket may be a Hash allocated by this read, so it
            // leaves the protect frame as its result, which the frame
            // keeps rooted past its arena restore.
            mrb.protect(|mrb| {
                let mut out = sys::mrb_value::zeroed();
                let mut kwargs = capture_all_kwargs(&mut out);
                // SAFETY: as `O::read`; the `":"` format reads the
                // keyword dict through the `mrb_kwargs` input struct.
                // Capture-all sends every pair to `rest`, which mruby
                // fills with an empty Hash when none were passed, so
                // `out` is Hash-tagged.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        Self::FMT.as_ptr(),
                        &mut kwargs as *mut sys::mrb_kwargs,
                    );
                }
                // SAFETY: capture-all guarantees the bucket is a Hash value.
                unsafe { crate::Hash::from_value_unchecked(Value::from_raw_unchecked(out)) }
            })
        }
    }

    /// `mrb_get_args(mrb, "n*:&", &sym, &argv, &argc, &kwargs, &block)` —
    /// read a leading symbol, a rest array, the keyword `Hash` bucket, and
    /// the block slot in one read. The shape a signature-blind
    /// `method_missing` proxy needs: the `:` specifier keeps the caller's
    /// keyword arguments in their own `Hash` (empty rather than nil when
    /// none were passed) instead of folding them into the rest, while an
    /// explicit positional `Hash` stays in the rest. An absent block
    /// decodes as nil. Checked, then read, as `NRest` is.
    pub struct NRestKwBlock;
    impl Format for NRestKwBlock {
        type Output<'a> = (sys::mrb_sym, &'a [Value], crate::Hash, Value);
        const FMT: &'static core::ffi::CStr = c"n*:&";

        fn read(mrb: &Mrb) -> Result<(sys::mrb_sym, &[Value], crate::Hash, Value), Error> {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: sys::mrb_int = 0;
            let mut out = sys::mrb_value::zeroed();
            let mut kwargs = capture_all_kwargs(&mut out);
            let mut block_raw = sys::mrb_value::zeroed();
            read_frame(mrb, |mrb| {
                // SAFETY: as below, with the rest left uncopied; the
                // bucket this check fills is discarded.
                unsafe {
                    sys::mrb_get_args(
                        mrb.as_ptr(),
                        c"n*!:&".as_ptr(),
                        &mut sym as *mut sys::mrb_sym,
                        &mut argv as *mut *const sys::mrb_value,
                        &mut argc as *mut sys::mrb_int,
                        &mut kwargs as *mut sys::mrb_kwargs,
                        &mut block_raw as *mut sys::mrb_value,
                    );
                }
            })?;
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
            let kw = unsafe { crate::Hash::from_value_unchecked(Value::from_raw_unchecked(out)) };
            Ok((
                sym,
                slice_from_argv(argv, argc),
                kw,
                Value::from_raw_unchecked(block_raw),
            ))
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
