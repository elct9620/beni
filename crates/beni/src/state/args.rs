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
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the `&self` borrow. The raise
            // on a wrong argument count long-jumps to the Ruby caller,
            // which the `-1` bridge frame is the contract for.
            Value::from_raw(unsafe { sys::mrb_get_arg1(self.as_ptr()) })
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Whether the current call was passed a block. A plain boolean
    /// question about the current call — `true` when a block was given,
    /// `false` otherwise. Total: it never raises. Mirrors magnus's
    /// `Ruby::block_given_p`.
    #[inline]
    pub fn block_given(&self) -> bool {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the `&self` borrow; the read is
            // total — it inspects the current call and never raises.
            unsafe { sys::mrb_block_given_p(self.as_ptr()) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }

    /// Read the number of arguments passed to the call frame, splat
    /// arguments counted as their expanded length. Does not raise.
    #[inline]
    pub fn argc(&self) -> sys::mrb_int {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the `&self` borrow; the read is
            // total — it never raises.
            unsafe { sys::mrb_get_argc(self.as_ptr()) }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

/// Zero-sized marker types implementing `Format`. Each marker maps
/// one mruby format string to a typed Rust return.
pub mod format {
    #[cfg(mruby_linked)]
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
        }
    }

    /// `mrb_get_args(mrb, "n*", &sym, &argv, &argc)` — read a leading
    /// symbol followed by a rest array.
    pub struct NRest;
    impl Format for NRest {
        type Output<'a> = (sys::mrb_sym, &'a [Value]);
        const FMT: &'static core::ffi::CStr = c"n*";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value]) {
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
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
            #[cfg(mruby_linked)]
            {
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
            #[cfg(not(mruby_linked))]
            {
                let _ = mrb;
                crate::not_linked()
            }
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
#[cfg(mruby_linked)]
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

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::format::{Io, NRest, Rest, RestBlock, Str, S};
    use super::*;

    /// Registered through `method!(rest_count, -1)`: reads the rest
    /// array via the `"*"` format and returns its length as an mruby
    /// Integer.
    fn rest_count(mrb: &Mrb, _self: Value) -> Value {
        let args = mrb.get_args::<Rest>();
        Value::from_int(mrb, args.len() as sys::mrb_int)
    }

    /// Registered through `method!(io_first, -1)`: reads the leading
    /// `"i"` integer and the trailing `"o"` object, returning the
    /// integer only when the object slot survived as `99` — so a read
    /// that overruns the integer slot into the adjacent object fails
    /// the assertion instead of passing.
    fn io_first(mrb: &Mrb, _self: Value) -> Value {
        use crate::FromValue;
        let (n, val) = mrb.get_args::<Io>();
        if i32::from_value(val) == Some(99) {
            Value::from_int(mrb, n)
        } else {
            Value::from_int(mrb, -1)
        }
    }

    /// Registered through `method!(nrest_after_sym, -1)`: reads the
    /// `"n"` leading symbol and the `"*"` rest array, returning the
    /// rest length only when the symbol decoded as `:tag` — so a read
    /// that folds the symbol into the rest array (or shifts the count)
    /// fails the assertion instead of passing.
    fn nrest_after_sym(mrb: &Mrb, _self: Value) -> Value {
        let (sym, rest) = mrb.get_args::<NRest>();
        if sym == mrb.intern_cstr(c"tag") {
            Value::from_int(mrb, rest.len() as sys::mrb_int)
        } else {
            Value::from_int(mrb, -1)
        }
    }

    // The `"*"` count out-param is written by mruby through `mrb_int*`
    // (`GET_ARG(mrb_int*)` in vendor/mruby/src/class.c). Typing it
    // narrower compiles under MRB_INT32 but corrupts the stack under
    // 64-bit mrb_int — a width coincidence the repo's validation
    // config cannot see. Exercising the full bridge → get_args →
    // count path under whatever ABI the linked archive uses keeps
    // that coincidence from coming back (`rake rust:test:default`
    // runs this against an upstream-default 64-bit-mrb_int archive).
    #[test]
    fn rest_format_reads_the_argc_mruby_writes() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"rest_count", crate::method!(rest_count, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let args = [
            Value::from_int(&mrb, 1),
            Value::from_int(&mrb, 2),
            Value::from_int(&mrb, 3),
        ];
        let count = receiver
            .funcall(&mrb, c"rest_count", &args)
            .expect("the bridge must not raise");

        assert!(count.is_integer(), "bridge must return an Integer");
        assert_eq!(unsafe { count.unbox_integer() }, 3);
    }

    // The `"i"` out-param is written by mruby as an `mrb_int` (8 bytes
    // under the default 64-bit-mrb_int archive). A narrower out-param
    // would write past its slot into the adjacent object value — the
    // same width coincidence the rest test guards, reached through
    // `"i"` rather than the `"*"` count. Asserting both the integer and
    // the trailing object survive under `rake rust:test:default` keeps
    // the `sys::mrb_int` out-param honest.
    #[test]
    fn io_format_reads_the_int_mruby_writes() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"io_first", crate::method!(io_first, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let args = [Value::from_int(&mrb, 7), Value::from_int(&mrb, 99)];
        let got = receiver
            .funcall(&mrb, c"io_first", &args)
            .expect("the bridge must not raise");

        assert!(got.is_integer(), "bridge must return an Integer");
        assert_eq!(
            unsafe { got.unbox_integer() },
            7,
            "a -1 means the trailing object slot did not survive the `\"i\"` read"
        );
    }

    // The `"n*"` read splits a leading symbol off before the rest
    // array. The symbol argument is supplied from Ruby — the typed
    // surface has no symbol-value constructor — so the call is driven
    // through a compiled fragment, as the break test does.
    #[test]
    fn nrest_format_splits_the_leading_symbol() {
        use crate::{Ccontext, FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(
                &mrb,
                c"nrest_after_sym",
                crate::method!(nrest_after_sym, -1),
            )
            .expect("registering the bridge must succeed");

        let recv = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let slot = mrb.intern_cstr(c"$beni_nrest_recv");
        mrb.gv_set(slot, recv);

        let cxt = Ccontext::new(&mrb, c"nrest_test.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"$beni_nrest_recv.nrest_after_sym(:tag, 1, 2, 3)");

        assert!(
            mrb.pending_exc().is_nil(),
            "the n* read must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(i32::from_value(got), Some(3));
    }

    /// Registered through `method!(s_echo, -1)`: reads the `"S"` String
    /// argument and returns it unchanged.
    fn s_echo(mrb: &Mrb, _self: Value) -> Value {
        mrb.get_args::<S>()
    }

    /// Registered through `method!(str_echo, -1)`: reads the `"s"`
    /// byte slice and copies it back into a fresh String, so the test
    /// verifies both the pointer and the length survived the read.
    fn str_echo(mrb: &Mrb, _self: Value) -> Value {
        let bytes = mrb.get_args::<Str>();
        mrb.str_new(bytes).as_value()
    }

    /// Registered through `method!(rest_block_report, -1)`: reads the
    /// `"*&"` rest array and block slot, returning the rest length when
    /// a block was given and `-1` otherwise — so a read that folds the
    /// block into the rest (or misplaces it) fails the assertion.
    fn rest_block_report(mrb: &Mrb, _self: Value) -> Value {
        let (rest, block) = mrb.get_args::<RestBlock>();
        if block.is_nil() {
            Value::from_int(mrb, -1)
        } else {
            Value::from_int(mrb, rest.len() as sys::mrb_int)
        }
    }

    #[test]
    fn s_format_reads_a_string_argument() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"s_echo", crate::method!(s_echo, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"s_echo", &[mrb.str_new(b"hello").as_value()])
            .expect("the bridge must not raise");

        assert!(got.is_string(), "the `\"S\"` read yields a String value");
        assert_eq!(got.to_string(&mrb), "hello");
    }

    #[test]
    fn str_format_reads_a_string_as_bytes() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"str_echo", crate::method!(str_echo, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"str_echo", &[mrb.str_new(b"hello").as_value()])
            .expect("the bridge must not raise");

        // The echoed String equals the input only if both the byte
        // pointer and the length were read correctly.
        assert_eq!(got.to_string(&mrb), "hello");
    }

    #[test]
    fn rest_block_format_splits_rest_from_block() {
        use crate::{Ccontext, FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(
                &mrb,
                c"rest_block_report",
                crate::method!(rest_block_report, -1),
            )
            .expect("registering the bridge must succeed");

        let recv = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let slot = mrb.intern_cstr(c"$beni_rest_block_recv");
        mrb.gv_set(slot, recv);

        let cxt = Ccontext::new(&mrb, c"rest_block_test.rb")
            .expect("allocating the compile context must succeed");

        // A block is given: the three positionals land in the rest
        // array, the block in its own slot — rest length 3.
        let with_block = cxt.load_nstring(b"$beni_rest_block_recv.rest_block_report(1, 2, 3) { }");
        assert!(
            mrb.pending_exc().is_nil(),
            "the *& read must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(i32::from_value(with_block), Some(3));

        // No block: the slot decodes as nil.
        let without_block = cxt.load_nstring(b"$beni_rest_block_recv.rest_block_report(1, 2)");
        assert_eq!(i32::from_value(without_block), Some(-1));
    }

    /// Registered through `method!(arg1_echo, -1)`: reads the single
    /// required argument via `Mrb::arg1` and returns it unchanged.
    fn arg1_echo(mrb: &Mrb, _self: Value) -> Value {
        mrb.arg1()
    }

    /// Registered through `method!(block_report, -1)`: returns whether a
    /// block was passed to the call as a Ruby boolean, read via
    /// `Mrb::block_given`.
    fn block_report(mrb: &Mrb, _self: Value) -> Value {
        use crate::IntoValue;
        mrb.block_given().into_value(mrb)
    }

    /// Registered through `method!(argc_report, -1)`: returns the
    /// argument count read via `Mrb::argc` as an mruby Integer.
    fn argc_report(mrb: &Mrb, _self: Value) -> Value {
        Value::from_int(mrb, mrb.argc())
    }

    /// Registered through `method!(argv_sum, -1)`: reads the whole
    /// positional argument array via `Mrb::argv` and returns the sum of
    /// the arguments, decoded as integers. Summing every slot — not just
    /// the first — fails the assertion if the slice length or any element
    /// is wrong, and the no-argument call exercises the empty-slice path
    /// (sum 0).
    fn argv_sum(mrb: &Mrb, _self: Value) -> Value {
        use crate::FromValue;
        let sum: sys::mrb_int = mrb
            .argv()
            .iter()
            .map(|v| sys::mrb_int::from(i32::from_value(*v).unwrap_or(0)))
            .sum();
        Value::from_int(mrb, sum)
    }

    #[test]
    fn arg1_reads_the_single_argument() {
        use crate::{FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"arg1_echo", crate::method!(arg1_echo, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let got = receiver
            .funcall(&mrb, c"arg1_echo", &[Value::from_int(&mrb, 42)])
            .expect("the single-argument read must not raise");
        assert_eq!(i32::from_value(got), Some(42));
    }

    #[test]
    fn arg1_raises_argument_error_on_wrong_count() {
        use crate::{Error, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"arg1_echo", crate::method!(arg1_echo, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        // Two positionals: `mrb_get_arg1` raises ArgumentError rather
        // than returning the first — the strict-count contract.
        let args = [Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)];
        let err = receiver
            .funcall(&mrb, c"arg1_echo", &args)
            .expect_err("a non-single argument count must surface as Err");
        match err {
            Error::Exception(exc) => assert_eq!(exc.classname(&mrb), "ArgumentError"),
            Error::Panic(_) => panic!("a wrong argument count must raise, not panic"),
        }
    }

    #[test]
    fn argc_reads_the_argument_count() {
        use crate::{FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"argc_report", crate::method!(argc_report, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let args = [
            Value::from_int(&mrb, 1),
            Value::from_int(&mrb, 2),
            Value::from_int(&mrb, 3),
        ];
        let got = receiver
            .funcall(&mrb, c"argc_report", &args)
            .expect("the count read must not raise");
        assert_eq!(i32::from_value(got), Some(3));
    }

    #[test]
    fn argv_reads_the_whole_argument_array() {
        use crate::{FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"argv_sum", crate::method!(argv_sum, -1))
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");

        // Several arguments: the body reads every slot and sums them, so
        // a short or misread slice would not total 60.
        let args = [
            Value::from_int(&mrb, 10),
            Value::from_int(&mrb, 20),
            Value::from_int(&mrb, 30),
        ];
        let got = receiver
            .funcall(&mrb, c"argv_sum", &args)
            .expect("the array read must not raise");
        assert_eq!(i32::from_value(got), Some(60));

        // No arguments: `argv` yields an empty slice, summed to 0,
        // exercising the `argc == 0` path without forming a slice from
        // the call frame pointer.
        let empty = receiver
            .funcall(&mrb, c"argv_sum", &[])
            .expect("the empty array read must not raise");
        assert_eq!(i32::from_value(empty), Some(0));
    }

    #[test]
    fn block_given_reports_whether_a_block_was_passed() {
        use crate::{Ccontext, FromValue, Module};

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(&mrb, c"block_report", crate::method!(block_report, -1))
            .expect("registering the bridge must succeed");

        let recv = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let slot = mrb.intern_cstr(c"$beni_block_recv");
        mrb.gv_set(slot, recv);

        // A block is supplied from Ruby — the typed surface has no
        // block-value constructor — so the call is driven through a
        // compiled fragment, as the *& read test does.
        let cxt = Ccontext::new(&mrb, c"block_given_test.rb")
            .expect("allocating the compile context must succeed");

        // No block: the predicate reports false.
        let without = cxt.load_nstring(b"$beni_block_recv.block_report");
        assert!(
            mrb.pending_exc().is_nil(),
            "the predicate must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(bool::from_value(without), Some(false));

        // A block is given: the predicate reports true.
        let with = cxt.load_nstring(b"$beni_block_recv.block_report { }");
        assert_eq!(bool::from_value(with), Some(true));
    }

    /// Registered through `method!(rest_borrowed_survives_reentry, -1)`:
    /// reads the `"*"` rest array as a borrowed slice, then re-enters the
    /// VM while holding it — compiling and running a fragment that
    /// allocates thousands of objects and recurses 400 frames deep — and
    /// only then joins the elements. Pins the rest-form contract that the
    /// borrow stays valid across VM re-entry: mruby backs it with a
    /// GC-arena-rooted copy, so a read that had dangled would surface as a
    /// corrupted join rather than the original bytes.
    fn rest_borrowed_survives_reentry(mrb: &Mrb, _self: Value) -> Value {
        let rest = mrb.get_args::<Rest>();
        // Re-enter the VM while holding the borrow: compilation allocates
        // heavily and the recursion grows the value stack. The rest borrow
        // survives because it views an arena-backed copy, not the live stack.
        let cxt = crate::Ccontext::new(mrb, c"reentry_probe.rb").expect("compile context");
        cxt.load_nstring(
            b"def __probe_deep(n); return 0 if n <= 0; Array.new(16){ 'y' * 40 }; __probe_deep(n - 1); end; __probe_deep(400)",
        );
        // The probe must actually run: a swallowed compile or runtime error
        // would leave the value stack unstressed, letting a dangling read slip
        // through as a false pass rather than exercising the re-entry contract.
        assert!(
            mrb.pending_exc().is_nil(),
            "the re-entry probe must run cleanly: {}",
            mrb.pending_exc().to_string(mrb)
        );
        mrb.full_gc();
        let mut joined = String::new();
        for v in rest {
            joined.push_str(&v.to_string(mrb));
        }
        mrb.str_new(joined.as_bytes()).as_value()
    }

    #[test]
    fn rest_borrowed_slice_survives_vm_reentry() {
        use crate::Module;

        let mrb = crate::Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb.object_class();
        class
            .define_method(
                &mrb,
                c"rest_borrowed_survives_reentry",
                crate::method!(rest_borrowed_survives_reentry, -1),
            )
            .expect("registering the bridge must succeed");

        let receiver = class
            .obj_new(&mrb, &[])
            .expect("the receiver constructs without raising");
        let args = [
            mrb.str_new(b"al").as_value(),
            mrb.str_new(b"pha").as_value(),
        ];
        let got = receiver
            .funcall(&mrb, c"rest_borrowed_survives_reentry", &args)
            .expect("the borrowed read must not raise");

        assert_eq!(got.to_string(&mrb), "alpha");
    }
}
