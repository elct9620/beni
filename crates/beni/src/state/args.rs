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
//!
//! Rest-form variants borrow the call frame's argv buffer; the
//! lifetime is tied to `&self`, which the bridge body holds for the
//! duration of the C call. mruby may set the rest pointer to NULL
//! when the rest count is zero — `slice_from_argv` folds that into
//! an empty `&[Value]` so callers do not have to gate on NULL.
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
//! pub struct S;
//! impl Format for S {
//!     type Output<'a> = Value;
//!     const FMT: &'static core::ffi::CStr = c"S";
//!     fn read(mrb: &Mrb) -> Self::Output<'_> {
//!         // mrb_get_args(mrb, "S", &out) — see `format::O` for the pattern
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
    /// use beni_sys::format::{Io, Rest};
    /// let (fd, mode_val) = mrb.get_args::<Io>();
    /// let argv = mrb.get_args::<Rest>();
    /// ```
    #[inline]
    pub fn get_args<F: Format>(&self) -> F::Output<'_> {
        F::read(self)
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

    /// `mrb_get_args(mrb, "*", &argv, &argc)` — read the rest array
    /// as a borrowed slice into the call frame.
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
    use super::format::{Io, NRest, Rest};
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

        let receiver = class.obj_new(&mrb, &[]);
        let args = [
            Value::from_int(&mrb, 1),
            Value::from_int(&mrb, 2),
            Value::from_int(&mrb, 3),
        ];
        let count = receiver.call(&mrb, c"rest_count", &args);

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

        let receiver = class.obj_new(&mrb, &[]);
        let args = [Value::from_int(&mrb, 7), Value::from_int(&mrb, 99)];
        let got = receiver.call(&mrb, c"io_first", &args);

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

        let recv = class.obj_new(&mrb, &[]);
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
}
