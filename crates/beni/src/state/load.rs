//! Source and RITE bytecode loaders on `Mrb`.
//!
//! Inherent methods that compile Ruby source — or drop a compiled
//! blob — into the live mruby VM and run its top-level Proc. The
//! `compiler` feature carries the source loader; the bytecode loader
//! needs no compiler and stays outside it.

use crate::{Error, Mrb, Value};
use beni_sys as sys;

impl Mrb {
    /// Compile and run a slice of Ruby `source` without being given a
    /// compile context, yielding the program's result value.
    ///
    /// A failure surfaces exactly as it does under a caller's context:
    /// source that does not parse comes back `Err(Error::Syntax)`
    /// carrying the compiler's diagnostic, and a codegen failure or a
    /// raise comes back `Err(Error::Exception)` with the pending
    /// exception cleared from the handle.
    ///
    /// The source carries its own length, so it needs no terminating
    /// NUL and the bytes need not be valid UTF-8.
    ///
    /// The load borrows an unnamed `Ccontext` and releases it on the
    /// way out, so it differs from one under a caller's context only
    /// where the context is what carries the difference: no filename
    /// is stamped, so a raised exception has no source-line backtrace,
    /// and the warnings the load produced go with the context a caller
    /// never holds. Reach for a `Ccontext` to keep either.
    #[cfg(feature = "compiler")]
    pub fn load_string(&self, source: &[u8]) -> Result<Value, crate::Error> {
        let Some(cxt) = crate::Ccontext::unnamed(self) else {
            // The context allocator answers NULL rather than raising,
            // and a load that never reached the compiler has the same
            // nothing to report as one whose compiler recorded no
            // diagnostic.
            return Err(crate::Error::Syntax(crate::ParseMessage::unrecorded()));
        };
        cxt.load_nstring(source)
    }

    /// Load and run a precompiled bytecode blob at the interpreter's
    /// top level, yielding the program's result value.
    ///
    /// The blob is the form `Proc::dump` answers. A blob mruby cannot
    /// read as a program comes back `Err(Error::Exception)` carrying a
    /// `ScriptError` whose message names which structural check failed,
    /// and nothing runs; an exception the program raises while it runs
    /// comes back the same way, carrying that exception with the
    /// pending exception cleared from the handle.
    ///
    /// The result is a value like any other, so carrying it past the
    /// arena scope that produced it needs a `GcRoot`.
    pub fn load_bytecode(&self, bytes: &[u8]) -> Result<Value, Error> {
        // SAFETY: bytes pointer is valid for the synchronous call.
        let irep = unsafe {
            sys::mrb_read_irep_buf(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        };

        if irep.is_null() {
            // `mrb_read_irep_buf` answers NULL under exactly the
            // condition mruby's own loader reports as a `ScriptError`
            // (`vendor/mruby/src/load.c:756,764`), so the error carries
            // that class — with the structural check named, which
            // mruby's one flat message does not distinguish.
            let script_error = self.class_get(c"ScriptError")?;
            return Err(Error::new(self, script_error, structural_failure(bytes)));
        }

        // Mirror mruby's static `load_irep` body: wrap the IREP in
        // a top-level Proc, hand IREP ownership to the Proc via
        // decref, then run.
        // SAFETY: `irep` was just returned non-null by
        // mrb_read_irep_buf; `mrb` is alive.
        let proc_ = unsafe { sys::mrb_proc_new_func(self.as_ptr(), irep) };
        // SAFETY: `proc_` came from mrb_proc_new and is alive until
        // the matching mrb_top_run consumes it.
        unsafe { (*proc_).c = core::ptr::null_mut() };
        // SAFETY: hands IREP ownership to the Proc.
        unsafe { sys::mrb_irep_decref(self.as_ptr(), irep) };
        // SAFETY: `mrb` is alive.
        let top_self = unsafe { sys::mrb_top_self(self.as_ptr()) };
        // SAFETY: top-level Proc execution; any raise sets mrb->exc,
        // which `outcome` reads back.
        let value = Value::from_raw(unsafe { sys::mrb_top_run(self.as_ptr(), proc_, top_self, 0) });
        self.outcome(value)
    }
}

/// Which structural check a blob mruby could not read as a program
/// failed, read from the RITE binary header (`mruby/dump.h`: ident in
/// bytes 0-3, format version in bytes 4-7). The constants come from
/// bindgen-emitted `RITE_BINARY_IDENT` / `RITE_BINARY_FORMAT_VER`
/// (each a 5-byte slice with a trailing NUL, so the first 4 bytes are
/// what the header carries).
fn structural_failure(bytes: &[u8]) -> &'static str {
    if bytes.len() < core::mem::size_of::<sys::rite_binary_header>() {
        return "bytecode shorter than RITE binary header";
    }
    if bytes[..4] != sys::RITE_BINARY_IDENT[..4] {
        return "bytecode header is not RITE format";
    }
    if bytes[4..8] != sys::RITE_BINARY_FORMAT_VER[..4] {
        return "bytecode RITE version mismatch";
    }
    "bytecode body failed structural validation"
}
