//! Source and RITE bytecode loaders on `Mrb`.
//!
//! Inherent methods that compile Ruby source — or drop a compiled
//! blob — into the live mruby VM and run its top-level Proc.

use crate::{Error, Mrb, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

impl Mrb {
    /// Compile and run a slice of Ruby `source` with no compile
    /// context, yielding the program's result value. A failure on any
    /// path — a parse error, a codegen error, or an exception raised
    /// while the program runs — comes back `Err(Error::Exception)`
    /// with the pending exception cleared from the handle.
    ///
    /// The source carries its own length, so it needs no terminating
    /// NUL and the bytes need not be valid UTF-8. The context-free
    /// counterpart to `Ccontext::load_nstring`: it stamps no filename
    /// (so a raised exception carries no source-line backtrace) and
    /// surfaces a failure as an `Err` rather than leaving the pending
    /// exception on the handle.
    pub fn load_string(&self, source: &[u8]) -> Result<Value, Error> {
        #[cfg(not(mruby_linked))]
        {
            let _ = source;
            crate::not_linked()
        }
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the &self borrow; `source` is
            // borrowed for the synchronous call and `mrb_load_nstring`
            // retains no reference past return. The load path runs
            // Ruby but parks any failure in `mrb->exc` rather than
            // long-jumping, so no `protect` frame is needed.
            let value = Value::from_raw(unsafe {
                sys::mrb_load_nstring(
                    self.as_ptr(),
                    source.as_ptr() as *const core::ffi::c_char,
                    source.len(),
                )
            });
            let exc = self.pending_exc();
            if exc.is_nil() {
                Ok(value)
            } else {
                self.clear_exc();
                Err(Error::Exception(exc))
            }
        }
    }

    /// `mrb_load_irep_buf(mrb, buf, size)` — load and evaluate a
    /// precompiled RITE bytecode blob. On a malformed blob mruby
    /// sets `mrb->exc`; callers should inspect via
    /// `Mrb::pending_exc` before continuing.
    #[inline]
    pub fn load_irep_buf(&self, bytes: &[u8]) -> Value {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive; `bytes` is borrowed for the
            // synchronous call.
            Value::from_raw(unsafe {
                sys::mrb_load_irep_buf(
                    self.as_ptr(),
                    bytes.as_ptr() as *const core::ffi::c_void,
                    bytes.len(),
                )
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = bytes;
            crate::not_linked()
        }
    }

    /// Load + validate + execute a precompiled bytecode blob.
    /// Returns 0 on success and 1 on structural failure (RITE
    /// version drift, corrupt or non-RITE body). Top-level
    /// exceptions from a successful load are left in `mrb->exc` for
    /// downstream extraction.
    ///
    /// Wraps the RITE parse step (which mruby keeps separate from
    /// execution via `mrb_read_irep_buf`) with arena bracketing and
    /// a structural-failure classifier; on parse failure a
    /// `RuntimeError` is synthesised under `mrb->exc` so the
    /// caller's pending-exception flow sees a normal exception. The
    /// classifier reads the RITE binary header directly from
    /// `bytes` so the diagnostic distinguishes "shorter than
    /// header" / "wrong ident" / "version mismatch" / "corrupt
    /// body".
    pub fn load_bytecode(&self, bytes: &[u8]) -> core::ffi::c_int {
        #[cfg(not(mruby_linked))]
        {
            let _ = bytes;
            crate::not_linked()
        }
        #[cfg(mruby_linked)]
        {
            self.load_bytecode_linked(bytes)
        }
    }

    /// Linked-mode body of `Mrb::load_bytecode`, split out because the
    /// multi-step arena/IREP dance reads better without an extra cfg
    /// indentation level.
    #[cfg(mruby_linked)]
    fn load_bytecode_linked(&self, bytes: &[u8]) -> core::ffi::c_int {
        // mruby/irep.h documents that `mrb_load_irep*` calls retain
        // one RProc per invocation in the arena; bracketing with
        // save/restore keeps multi-snippet preload cost bounded.
        // mrb->exc is itself a GC root, so any synthesised exception
        // below survives the restore.
        // SAFETY: `self` is alive by the &self borrow.
        let ai = unsafe { sys::mrb_gc_arena_save_func(self.as_ptr()) };

        // SAFETY: bytes pointer is valid for the synchronous call.
        let irep = unsafe {
            sys::mrb_read_irep_buf(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        };

        if irep.is_null() {
            // Version drift, corrupt body, or non-RITE input. The
            // synthesised exception surfaces through `mrb->exc`
            // exactly like a native raise.
            self.set_bytecode_exc(classify_structural_failure(bytes));
            // SAFETY: arena index from the matching save above.
            unsafe { sys::mrb_gc_arena_restore_func(self.as_ptr(), ai) };
            return 1;
        }

        // Mirror mruby's static `load_irep` body: wrap the IREP in
        // a top-level Proc, hand IREP ownership to the Proc via
        // decref, then run. Any top-level raise sets mrb->exc and
        // the caller's existing path picks it up.
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
        // SAFETY: top-level Proc execution; any raise sets mrb->exc.
        unsafe { sys::mrb_top_run(self.as_ptr(), proc_, top_self, 0) };
        // SAFETY: arena index from the matching save above.
        unsafe { sys::mrb_gc_arena_restore_func(self.as_ptr(), ai) };
        0
    }

    /// Set `mrb->exc` to a freshly synthesised `RuntimeError` carrying
    /// `msg`. Used by `Mrb::load_bytecode` to surface structural
    /// failures from `mrb_read_irep_buf` (which signals failure by
    /// returning NULL without setting `mrb->exc`). The caller's
    /// existing pending-exception extraction picks the synthesised
    /// exception up uniformly with mruby-native raises.
    #[cfg(mruby_linked)]
    fn set_bytecode_exc(&self, msg: &str) {
        // SAFETY: `self` is alive; `c"RuntimeError"` is a static
        // NUL-terminated literal.
        let runtime_error = unsafe { sys::mrb_class_get(self.as_ptr(), c"RuntimeError".as_ptr()) };
        // SAFETY: `msg` is a Rust string slice borrowed for the
        // synchronous call; mruby copies the bytes into a new
        // exception object.
        let err = Value::from_raw(unsafe {
            sys::mrb_exc_new(
                self.as_ptr(),
                runtime_error,
                msg.as_ptr() as *const core::ffi::c_char,
                msg.len() as sys::mrb_int,
            )
        });
        self.set_pending_exc(err);
    }
}

/// Classify a structural `mrb_read_irep_buf` failure by inspecting
/// the RITE binary header (`mruby/dump.h`: ident in bytes 0–3,
/// format version in bytes 4–7). Returns a stable diagnostic the
/// caller wraps in a `RuntimeError`. The constants come from
/// bindgen-emitted `RITE_BINARY_IDENT` / `RITE_BINARY_FORMAT_VER`
/// (each is a 5-byte slice with a trailing NUL — compare the first
/// 4 bytes against the magic / version bytes the header actually
/// carries).
#[cfg(mruby_linked)]
fn classify_structural_failure(bytes: &[u8]) -> &'static str {
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

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{Error, FromValue, Mrb};
    use beni_sys as sys;

    const HEADER_LEN: usize = core::mem::size_of::<sys::rite_binary_header>();

    #[test]
    fn load_string_evaluates_a_valid_expression_to_its_value() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let got = mrb
            .load_string(b"1 + 2")
            .expect("a valid expression must come back Ok");

        assert_eq!(i32::from_value(got), Some(3));
        assert!(
            mrb.pending_exc().is_nil(),
            "a successful eval must leave no pending exception"
        );
    }

    #[test]
    fn load_string_surfaces_a_raising_script_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .load_string(b"raise 'kaboom'")
            .expect_err("a raising script must come back Err");

        match err {
            Error::Exception(_) => assert!(err.message(&mrb).contains("kaboom")),
            Error::Panic(_) => panic!("a Ruby raise must surface as Error::Exception"),
        }
        assert!(
            mrb.pending_exc().is_nil(),
            "the pending exception must be cleared as the Err crosses out"
        );
    }

    #[test]
    fn load_string_surfaces_a_syntax_error_as_err() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .load_string(b"def (")
            .expect_err("unparseable source must come back Err");

        assert!(matches!(err, Error::Exception(_)));
        assert!(
            mrb.pending_exc().is_nil(),
            "the pending exception must be cleared as the Err crosses out"
        );
    }

    /// The synthesised `RuntimeError` parked under `mrb->exc`, rendered.
    fn exc_message(mrb: &Mrb) -> String {
        let exc = mrb.pending_exc();
        assert!(
            !exc.is_nil(),
            "a structural failure must synthesise mrb->exc"
        );
        exc.to_string(mrb)
    }

    #[test]
    fn load_bytecode_classifies_a_blob_shorter_than_the_header() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        assert_eq!(mrb.load_bytecode(b"RITE"), 1);
        assert!(exc_message(&mrb).contains("shorter than RITE binary header"));
    }

    #[test]
    fn load_bytecode_classifies_a_non_rite_ident() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        assert_eq!(mrb.load_bytecode(&[b'X'; HEADER_LEN]), 1);
        assert!(exc_message(&mrb).contains("not RITE format"));
    }

    #[test]
    fn load_bytecode_classifies_a_rite_version_mismatch() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let mut blob = [0u8; HEADER_LEN];
        blob[..4].copy_from_slice(&sys::RITE_BINARY_IDENT[..4]);
        blob[4..8].copy_from_slice(b"0000");

        assert_eq!(mrb.load_bytecode(&blob), 1);
        assert!(exc_message(&mrb).contains("RITE version mismatch"));
    }

    #[test]
    fn load_bytecode_classifies_a_corrupt_body() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let mut blob = [0u8; HEADER_LEN + 8];
        blob[..4].copy_from_slice(&sys::RITE_BINARY_IDENT[..4]);
        blob[4..8].copy_from_slice(&sys::RITE_BINARY_FORMAT_VER[..4]);

        assert_eq!(mrb.load_bytecode(&blob), 1);
        assert!(exc_message(&mrb).contains("failed structural validation"));
    }
}
