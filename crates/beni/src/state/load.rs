//! Source and RITE bytecode loaders on `Mrb`.
//!
//! Inherent methods that compile Ruby source — or read a compiled
//! blob — into the live mruby VM and run its top-level Proc. The
//! `compiler` feature carries the source loader; the bytecode read and
//! load need no compiler and stay outside it.

use crate::sys::AsRawValue;
use crate::{Error, Mrb, Proc, Value};
use beni_sys as sys;

impl Mrb {
    /// Compile and run Ruby `source` without being given a compile
    /// context, yielding the program's result converted to `T`. Mirrors
    /// magnus's `Ruby::eval`.
    ///
    /// A failure surfaces exactly as it does under a caller's context:
    /// source that does not parse comes back `Err(Error::Syntax)`
    /// carrying the compiler's diagnostic, and a codegen failure or a
    /// raise comes back `Err(Error::Exception)` with the pending
    /// exception cleared from the handle. A result `T` rejects comes
    /// back as that conversion's `Err`. Source that is not UTF-8 loads
    /// through `Ccontext::load_nstring`.
    ///
    /// The load borrows an unnamed `Ccontext` and releases it on the
    /// way out, so it differs from one under a caller's context only
    /// where the context is what carries the difference: no filename
    /// is stamped, so a raised exception has no source-line backtrace,
    /// and the warnings the load produced go with the context a caller
    /// never holds. Reach for a `Ccontext` to keep either.
    #[cfg(feature = "compiler")]
    pub fn eval<T: crate::TryConvert>(&self, source: &str) -> Result<T, crate::Error> {
        let Some(cxt) = crate::Ccontext::unnamed(self) else {
            // The context allocator answers NULL rather than raising,
            // and a load that never reached the compiler has the same
            // nothing to report as one whose compiler recorded no
            // diagnostic.
            return Err(crate::Error::Syntax(crate::ParseMessage::unrecorded()));
        };
        T::try_convert(cxt.load_nstring(source.as_bytes())?, self)
    }

    /// Read a precompiled bytecode blob as a program without running
    /// it, yielding the program as a `Proc`.
    ///
    /// The blob is the form `Proc::dump` answers. A blob mruby cannot
    /// read as a program comes back `Err(Error::Exception)` carrying a
    /// `ScriptError` whose message names which structural check failed.
    /// `Proc::call` runs the program at the interpreter's top level, so a
    /// raise from it comes back from that call, never from this read.
    ///
    /// The program is a value like any other, so carrying it past the
    /// arena scope that produced it needs a `GcRoot`.
    ///
    /// # Safety
    ///
    /// mruby validates the blob's structure, not the instructions it
    /// carries. The blob must be the unmodified output of `Proc::dump` or
    /// of mruby's compiler from this interpreter's mruby release, or one
    /// that fails structural validation; any other blob can reach memory
    /// outside the program once it runs.
    pub unsafe fn read_irep(&self, bytes: &[u8]) -> Result<Proc, Error> {
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
            let script_error = self.exception_script_error()?;
            return Err(Error::new(self, script_error, structural_failure(bytes)));
        }

        // Build the Proc as mruby's static `load_irep` does — wrap the
        // IREP, hand IREP ownership to the Proc via decref — and target
        // it at the top level as `mrb_load_exec` does.
        // SAFETY: `irep` was just returned non-null by
        // mrb_read_irep_buf; `mrb` is alive.
        let proc_ = unsafe { sys::mrb_proc_new_func(self.as_ptr(), irep) };
        // SAFETY: `proc_` came from mrb_proc_new and is alive.
        unsafe { (*proc_).c = core::ptr::null_mut() };
        // SAFETY: hands IREP ownership to the Proc.
        unsafe { sys::mrb_irep_decref(self.as_ptr(), irep) };
        self.target_top_level(proc_);
        // SAFETY: `proc_` is the live Proc just built, which
        // `mrb_obj_value` boxes Proc-tagged.
        Ok(unsafe {
            Proc::from_value_unchecked(Value::from_raw_unchecked(sys::mrb_obj_value(proc_.cast())))
        })
    }

    /// Read a precompiled bytecode blob and run it at the interpreter's
    /// top level, yielding the program's result value.
    ///
    /// A blob mruby cannot read fails as `read_irep` reports it, and
    /// nothing runs; an exception the program raises while it runs comes
    /// back `Err(Error::Exception)` carrying that exception with the
    /// pending exception cleared from the handle.
    ///
    /// The result is a value like any other, so carrying it past the
    /// arena scope that produced it needs a `GcRoot`.
    ///
    /// # Safety
    ///
    /// The blob carries the same obligation `read_irep` states.
    pub unsafe fn load_irep(&self, bytes: &[u8]) -> Result<Value, Error> {
        // SAFETY: the caller upholds the blob's obligation.
        let program = unsafe { self.read_irep(bytes)? };
        // SAFETY: `program` is a Proc, whose object header
        // `mrb_obj_ptr_func` reads.
        let proc_ = unsafe { sys::mrb_obj_ptr_func(program.as_raw()) } as *mut sys::RProc;
        // SAFETY: `mrb` is alive.
        let top_self = unsafe { sys::mrb_top_self(self.as_ptr()) };
        self.protect(|mrb| {
            // SAFETY: top-level Proc execution; a raise inside a running
            // VM long-jumps to this protect frame, and one at the top
            // level returns with mrb->exc set, which `outcome` reads.
            Value::from_raw_unchecked(unsafe { sys::mrb_top_run(mrb.as_ptr(), proc_, top_self, 0) })
        })
        .and_then(|value| self.outcome(value))
    }

    /// Point a program's Proc at `Object`, as mruby's source loader
    /// does before it runs one (`mrb_load_exec`). A Proc takes its target
    /// class from the frame it is created in, so a program created inside
    /// a method would otherwise define its constants in that method's
    /// class rather than at the top level.
    pub(crate) fn target_top_level(&self, proc_: *mut sys::RProc) {
        // SAFETY: `proc_` is a live Proc of this VM, and `object_class`
        // is the VM's own `Object`.
        unsafe {
            sys::mrb_proc_set_target_class_func(
                self.as_ptr(),
                proc_,
                sys::mrb_object_class(self.as_ptr()),
            )
        };
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
