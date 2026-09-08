//! RAII wrapper around mruby's `mrb_ccontext *`.
//!
//! A compile context stamps a filename onto everything compiled
//! through it, so the produced IREP carries `debug_info` and the
//! exceptions the program raises answer `Exception#backtrace` (see
//! `vendor/mruby/src/backtrace.c::pack_backtrace`). One context serves
//! any number of loads and carries the top-level local variables
//! across them, so successive loads see each other's locals.
//!
//! A context compiles source and runs it, or compiles it and stops,
//! handing back the program as a Proc for a caller who means to dump or
//! run it later. Which of the two happens is settled for that call
//! alone; the context keeps no memory of it.
//!
//! The context also captures the compiler's diagnostics instead of
//! letting mruby print them: a load hands back the first error as a
//! `ParseMessage` and the context keeps that load's warnings as more
//! of them, which is the only place either location surfaces.

use crate::{Error, Mrb, ParseMessage, Proc, Value};
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
        let cxt = Self::unnamed(mrb)?;
        // SAFETY: `mrb` is live; `cxt.raw` came from the matching
        // `mrb_ccontext_new`; `filename.as_ptr()` is a NUL-terminated
        // `*const c_char` by `CStr`'s invariant.
        unsafe { sys::mrb_ccontext_filename(mrb.as_ptr(), cxt.raw, filename.as_ptr()) };
        Some(cxt)
    }

    /// Allocate a context with no filename — everything a context
    /// gives a load except the stamp its backtraces are packed from.
    ///
    /// `Mrb::load_string` borrows one of these for a single load: a
    /// caller who gave no context still gets the compiler's
    /// diagnostics, which only a context can capture.
    pub(crate) fn unnamed(mrb: &'mrb Mrb) -> Option<Self> {
        // SAFETY: `mrb` is live by the borrow.
        let raw = unsafe { sys::mrb_ccontext_new(mrb.as_ptr()) };
        if raw.is_null() {
            return None;
        }
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
        let parser = self.parse(source)?;
        // SAFETY: `parser` parsed cleanly; `mrb_load_exec` takes
        // ownership of it, frees it, and runs the generated Proc.
        let value =
            Value::from_raw(unsafe { sys::mrb_load_exec(self.mrb.as_ptr(), parser, self.raw) });
        self.outcome(value)
    }

    /// Compile `source` under this context without running it, yielding
    /// the compiled program.
    ///
    /// The failures are the ones a load answers, for the same reasons:
    /// source that does not parse comes back `Err(Error::Syntax)`
    /// carrying the compiler's first recorded diagnostic, and a codegen
    /// failure comes back `Err(Error::Exception)`. The context's
    /// warnings are recorded either way.
    ///
    /// `Proc::call` runs the program at the interpreter's top level. It
    /// starts without the context's top-level local variables: those
    /// reach a program the context itself runs. The program is a value
    /// like any other, so carrying it past the arena scope that
    /// produced it needs a `GcRoot`.
    pub fn compile(&self, source: &[u8]) -> Result<Proc, Error> {
        let parser = self.parse(source)?;
        let value = self.outcome(self.generate(parser))?;
        // SAFETY: under `no_exec` a successful `mrb_load_exec` answers
        // `mrb_obj_value(proc)` for the Proc it generated
        // (`vendor/mruby/mrbgems/mruby-compiler/core/parse.y:7782`), and
        // the codegen failure that would answer otherwise left the
        // exception `outcome` has already returned.
        Ok(unsafe { Proc::from_value_unchecked(value) })
    }

    /// Hand the parser to mruby with this context stopped before it
    /// runs what it generates. Whether an operation stops is settled
    /// for that operation alone, so the flag is cleared on the way out
    /// and no later load reads it.
    fn generate(&self, parser: *mut sys::mrb_parser_state) -> Value {
        // SAFETY: `self.raw` came from `mrb_ccontext_new`; the raw
        // setter writes the bitfield through a pointer rather than
        // forming a `&mut` to memory mruby owns.
        unsafe { sys::mrb_ccontext::set_no_exec_raw(self.raw, true) };
        // SAFETY: `parser` parsed cleanly; `mrb_load_exec` takes
        // ownership of it and frees it.
        let value =
            Value::from_raw(unsafe { sys::mrb_load_exec(self.mrb.as_ptr(), parser, self.raw) });
        // SAFETY: as above.
        unsafe { sys::mrb_ccontext::set_no_exec_raw(self.raw, false) };
        value
    }

    /// What an operation that reached mruby answers: its value, or the
    /// exception it raised, cleared from the handle as it crosses out.
    fn outcome(&self, value: Value) -> Result<Value, Error> {
        let exc = self.mrb.pending_exc();
        if exc.is_nil() {
            Ok(value)
        } else {
            self.mrb.clear_exc();
            Err(Error::Exception(exc))
        }
    }

    /// Parse `source` under this context, recording the warnings it
    /// produced and handing back the parser for the caller to spend.
    /// Source that does not parse is released here and reported as the
    /// compiler's first recorded diagnostic.
    fn parse(&self, source: &[u8]) -> Result<*mut sys::mrb_parser_state, Error> {
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

        Ok(parser)
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
