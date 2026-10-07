//! Reads of the current call frame beside `scan_args`: the single
//! required argument, the arguments an any-arity method receives, the
//! name the call reached its method by, and whether a block was passed.

use crate::{Error, FromValue, Id, Mrb, ReprValue, Value};
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

    /// The name the running call reached its method by — the alias
    /// itself when called through one — so one function registered
    /// under several names tells them apart; `None` outside any method
    /// call. Total: it never raises. magnus names no counterpart.
    #[inline]
    pub fn mid(&self) -> Option<Id> {
        // SAFETY: `self` is alive by the `&self` borrow; the read only
        // loads the current call's method id.
        let mid = unsafe { sys::mrb_get_mid(self.as_ptr()) };
        (mid != 0).then(|| Id::from_raw_unchecked(mid))
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
    if keywords_given(mrb) {
        return with_call(mrb, |args, _| body(args));
    }
    body(ArgsCopy::new(frame_positionals(mrb), None).as_slice())
}

/// The call's positionals as `N` slots, read without a format parse when
/// the call passed no keywords and between `required` and `N` positionals
/// — the shapes a read of `required` positionals and `N - required`
/// optional ones accepts as they stand. Each slot the call left out holds
/// undef, as `mrb_get_args` leaves an omitted optional. Any other call
/// answers `None`, leaving the frame to `mrb_get_args`, which folds
/// keywords into a positional or raises mruby's own count error.
#[inline]
pub(crate) fn frame_args<const N: usize>(mrb: &Mrb, required: usize) -> Option<[Value; N]> {
    if keywords_given(mrb) {
        return None;
    }
    let positionals = frame_positionals(mrb);
    if positionals.len() == N {
        return positionals.try_into().ok();
    }
    if positionals.len() < required || positionals.len() > N {
        return None;
    }
    // SAFETY: an absent optional's undef slot is read only by the
    // bridge, which answers `None` for it and hands no Ruby code the value.
    let mut slots = [unsafe { crate::value::qundef().as_value() }; N];
    slots[..positionals.len()].copy_from_slice(positionals);
    Some(slots)
}

/// The call's `N` positionals and its block slot, read without a protect
/// frame: the uncopied-rest read accepts every count and folds a keyword
/// hash into the positionals as a fixed-count read does, so the count is
/// checked here and a mismatch answers the `ArgumentError` mruby's own
/// read raises for it.
#[inline]
pub(crate) fn frame_args_with_block<const N: usize>(
    mrb: &Mrb,
) -> Result<([Value; N], Value), Error> {
    let call = crate::scan_args::read_raw(mrb, false);
    match call.positionals.try_into() {
        Ok(positionals) => Ok((positionals, call.block)),
        Err(_) => Err(crate::scan_args::argnum_error(
            mrb,
            call.positionals.len(),
            N,
            Some(N),
        )),
    }
}

/// As `with_args`, also handing over the call's block: the `Proc` the
/// call passed, or `None` when it passed none.
pub(crate) fn with_call<R>(mrb: &Mrb, body: impl FnOnce(&[Value], Option<crate::Proc>) -> R) -> R {
    // A call without keywords is read without the keyword bucket, which
    // mruby fills with a fresh Hash on every call; with none to fold into
    // the positionals, that read leaves the frame as it was too.
    let call = crate::scan_args::read_raw(mrb, keywords_given(mrb));
    let block = crate::Proc::from_value(call.block);
    let keywords = call
        .keywords
        .filter(|keywords| !keywords.is_empty(mrb))
        .map(ReprValue::as_value);
    body(ArgsCopy::new(call.positionals, keywords).as_slice(), block)
}

/// The current call's positionals as they stand in its frame.
fn frame_positionals(mrb: &Mrb) -> &[Value] {
    // SAFETY: `mrb` is alive inside a C function's call; mruby's own
    // argument accessors read its current call info and never raise.
    unsafe {
        slice_from_argv(
            sys::mrb_get_argv(mrb.as_ptr()),
            sys::mrb_get_argc(mrb.as_ptr()),
        )
    }
}

/// Whether the current call passed keywords.
fn keywords_given(mrb: &Mrb) -> bool {
    // SAFETY: `mrb` is alive, so its current call info is too.
    unsafe { sys::mrb_ci_keywords_given_func(mrb.as_ptr()) }
}

/// A copy of a call's arguments, `positionals` followed by `keywords`,
/// out of the frame so a VM re-entry that moves its stack leaves them
/// valid. A short list is held inline and allocates nothing.
pub(crate) enum ArgsCopy {
    Inline {
        len: usize,
        values: [Value; INLINE_ARGS],
    },
    Heap(Vec<Value>),
}

impl ArgsCopy {
    #[inline]
    pub(crate) fn new(positionals: &[Value], keywords: Option<Value>) -> Self {
        let len = positionals.len() + usize::from(keywords.is_some());
        if len <= INLINE_ARGS {
            let mut values = [crate::value::qnil().as_value(); INLINE_ARGS];
            values[..positionals.len()].copy_from_slice(positionals);
            if let Some(keywords) = keywords {
                values[len - 1] = keywords;
            }
            ArgsCopy::Inline { len, values }
        } else {
            let mut values = Vec::with_capacity(len);
            values.extend_from_slice(positionals);
            values.extend(keywords);
            ArgsCopy::Heap(values)
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[Value] {
        match self {
            ArgsCopy::Inline { len, values } => &values[..*len],
            ArgsCopy::Heap(values) => values,
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
