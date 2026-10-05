//! Reads of the current call frame beside `scan_args`: the single
//! required argument, the arguments an any-arity method receives, and
//! whether a block was passed.

use crate::{Error, FromValue, Mrb, ReprValue, Value};
use beni_sys as sys;

/// Run a call-frame read under exception protection. `mrb_get_args`
/// raises for a call the read's shape does not accept; protected, that
/// raise comes back as the `Err` carrying mruby's exception, across
/// plain frames alone.
pub(crate) fn read_frame(mrb: &Mrb, read: impl FnOnce(&Mrb)) -> Result<(), Error> {
    mrb.protect(|mrb| {
        read(mrb);
        crate::value::qnil()
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
}

/// Arguments held inline before the copy spills to the heap.
const INLINE_ARGS: usize = 8;

/// Run `body` over a copy of the call's arguments: the positionals, then
/// the call's own keyword hash as one trailing value when it is
/// non-empty. The frame is read once and left as it was, so a later
/// frame read still finds the keywords; the copy stays valid whatever
/// `body` re-enters, the values being the frame's, kept alive for the
/// whole call.
pub(crate) fn with_args<R>(mrb: &Mrb, body: impl FnOnce(&[Value]) -> R) -> R {
    with_call(mrb, |args, _| body(args))
}

/// As `with_args`, also handing over the call's block: the `Proc` the
/// call passed, or `None` when it passed none.
pub(crate) fn with_call<R>(mrb: &Mrb, body: impl FnOnce(&[Value], Option<crate::Proc>) -> R) -> R {
    let call = crate::scan_args::read_raw(mrb, true);
    let block = crate::Proc::from_value(call.block);
    let keywords = call
        .keywords
        .filter(|keywords| !keywords.is_empty(mrb))
        .map(ReprValue::as_value);
    let len = call.positionals.len() + usize::from(keywords.is_some());
    if len <= INLINE_ARGS {
        let mut args = [crate::value::qnil().as_value(); INLINE_ARGS];
        args[..call.positionals.len()].copy_from_slice(call.positionals);
        if let Some(keywords) = keywords {
            args[len - 1] = keywords;
        }
        body(&args[..len], block)
    } else {
        let mut args = Vec::with_capacity(len);
        args.extend_from_slice(call.positionals);
        args.extend(keywords);
        body(&args, block)
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
