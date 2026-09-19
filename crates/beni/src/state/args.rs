//! Reads of the current call frame beside `scan_args`: the single
//! required argument, the argument count and array, and whether a block
//! was passed.

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

impl Mrb {
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

/// Cast a `mrb_get_args` rest-form `(*const mrb_value, mrb_int)` pair
/// into a borrowed `&[Value]`. mruby may set the pointer to NULL when
/// the rest count is zero; reading `len` bytes from NULL would be UB,
/// so the helper folds that into an empty slice.
///
/// The slice's lifetime is bound by the caller's `&self` borrow on
/// `Mrb` (the call frame that produced argv).
#[inline]
pub(crate) fn slice_from_argv<'a>(argv: *const sys::mrb_value, argc: sys::mrb_int) -> &'a [Value] {
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
pub(crate) fn capture_all_kwargs(out: *mut sys::mrb_value) -> sys::mrb_kwargs {
    sys::mrb_kwargs {
        num: 0,
        required: 0,
        table: core::ptr::null(),
        values: core::ptr::null_mut(),
        rest: out,
    }
}
