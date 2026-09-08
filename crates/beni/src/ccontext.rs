//! RAII wrapper around mruby's `mrb_ccontext *`.
//!
//! A compile context stamps a filename onto everything compiled
//! through it, so the produced IREP carries `debug_info` and the
//! exceptions the program raises answer `Exception#backtrace` (see
//! `vendor/mruby/src/backtrace.c::pack_backtrace`). One context serves
//! any number of loads and carries the top-level local variables
//! across them, so successive loads see each other's locals.
//!
//! The context also captures the compiler's diagnostics instead of
//! letting mruby print them: a load hands back the first error as a
//! `ParseMessage` and the context keeps that load's warnings as more
//! of them, which is the only place either location surfaces.

use crate::{Error, Mrb, ParseMessage, Value};
use beni_sys as sys;
use core::cell::RefCell;

/// Owned mruby compile context, tied to the lifetime of an `Mrb`.
///
/// The lifetime parameter prevents the context from outliving the
/// `mrb_state` that produced it: when `Drop` runs we still need
/// `self.mrb.as_ptr()` to call `mrb_ccontext_free`, and the borrow
/// checker keeps `Mrb` alive long enough.
///
/// The guard borrows the interpreter, so it stays on the thread that
/// made it:
///
/// ```compile_fail
/// fn carried<T: Send>() {}
/// carried::<beni::Ccontext<'static>>();
/// ```
pub struct Ccontext<'mrb> {
    mrb: &'mrb Mrb,
    raw: *mut sys::mrb_ccontext,
    /// The warnings the most recent load produced. A load takes
    /// `&self` so the interpreter can be reached from a registered
    /// method's frame; the cell is what lets it record on the way
    /// through.
    warnings: RefCell<Vec<ParseMessage>>,
}

impl<'mrb> Ccontext<'mrb> {
    /// Allocate a fresh compile context and stamp it with `filename`.
    /// Returns `None` when `mrb_ccontext_new` returns NULL.
    ///
    /// `mrb_ccontext_filename` interns the bytes, so the `&CStr`
    /// borrow only has to outlive this call.
    pub fn new(mrb: &'mrb Mrb, filename: &core::ffi::CStr) -> Option<Self> {
        // SAFETY: `mrb` is live by the borrow.
        let raw = unsafe { sys::mrb_ccontext_new(mrb.as_ptr()) };
        if raw.is_null() {
            return None;
        }
        // SAFETY: `mrb` is live; `raw` was just produced by the
        // matching `mrb_ccontext_new`; `filename.as_ptr()` is a
        // NUL-terminated `*const c_char` by `CStr`'s invariant.
        unsafe { sys::mrb_ccontext_filename(mrb.as_ptr(), raw, filename.as_ptr()) };
        // Capture is not a choice a caller gets: `load_nstring` reads
        // the parser's diagnostic buffer to build its `ParseMessage`,
        // and an uncaptured parser writes that buffer nothing. Capture
        // also keeps the diagnostic off the process's standard error,
        // which is the host's to write, not a library's.
        //
        // SAFETY: `raw` points at the context just allocated above;
        // the raw setter writes the bitfield through a pointer rather
        // than forming a `&mut` to memory mruby owns.
        unsafe { sys::mrb_ccontext::set_capture_errors_raw(raw, true) };
        Some(Self {
            mrb,
            raw,
            warnings: RefCell::new(Vec::new()),
        })
    }

    /// Compile and evaluate `source` under this context, yielding the
    /// program's result value. `source` is raw bytes (ptr + len), not
    /// NUL-terminated.
    ///
    /// Source that does not parse comes back `Err(Error::Syntax)`
    /// carrying the compiler's first recorded diagnostic; a codegen
    /// failure or a raise while the program runs comes back
    /// `Err(Error::Exception)` with the pending exception cleared from
    /// the handle. Only the exception answers a backtrace — a program
    /// that never compiled never ran.
    pub fn load_nstring(&self, source: &[u8]) -> Result<Value, Error> {
        // SAFETY: `self.mrb` is live by the borrow; `self.raw` was
        // produced by `mrb_ccontext_new` in `Self::new` and is owned
        // for the lifetime of `&self`; the source bytes outlive the
        // call because `mrb_parse_nstring` copies what it keeps.
        let parser = unsafe {
            sys::mrb_parse_nstring(
                self.mrb.as_ptr(),
                source.as_ptr() as *const core::ffi::c_char,
                source.len(),
                self.raw,
            )
        };
        if parser.is_null() {
            self.warnings.replace(Vec::new());
            return Err(Error::Syntax(ParseMessage::unrecorded()));
        }

        // SAFETY: `parser` is non-NULL and untouched since the parse;
        // the buffer outlives this read either way.
        let warnings = unsafe { ParseMessage::recorded(&(*parser).warn_buffer) };
        self.warnings.replace(warnings);

        // SAFETY: `parser` is non-NULL and untouched since the parse.
        let parsed = unsafe { (*parser).nerr == 0 && !(*parser).tree.is_null() };
        if !parsed {
            // SAFETY: as above; the buffer belongs to the live parser
            // and nothing mutates it between the parse and this read.
            let message = unsafe { ParseMessage::first_recorded(&(*parser).error_buffer) };
            // The failing parser is beni's to release. Handing it to
            // `mrb_load_exec` instead would have it format slot 0
            // unconditionally, which is not always a slot the compiler
            // wrote.
            //
            // SAFETY: `parser` is live and this is its only release.
            unsafe { sys::mrb_parser_free(parser) };
            return Err(Error::Syntax(message));
        }

        // SAFETY: `parser` parsed cleanly; `mrb_load_exec` takes
        // ownership of it, frees it, and runs the generated Proc.
        let value =
            Value::from_raw(unsafe { sys::mrb_load_exec(self.mrb.as_ptr(), parser, self.raw) });
        let exc = self.mrb.pending_exc();
        if exc.is_nil() {
            Ok(value)
        } else {
            self.mrb.clear_exc();
            Err(Error::Exception(exc))
        }
    }

    /// The warnings the compiler produced for the most recent load.
    ///
    /// Warnings do not change a load's outcome: a load that produces
    /// warnings and no error still yields its value. A context that
    /// has run no load, or whose most recent load warned about
    /// nothing, answers an empty list.
    pub fn warnings(&self) -> Vec<ParseMessage> {
        self.warnings.borrow().clone()
    }
}

impl Drop for Ccontext<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.mrb` is alive per the borrow; `self.raw` was
        // produced by `mrb_ccontext_new` and has not been freed yet
        // (`Self` is the sole owner).
        unsafe { sys::mrb_ccontext_free(self.mrb.as_ptr(), self.raw) };
    }
}
