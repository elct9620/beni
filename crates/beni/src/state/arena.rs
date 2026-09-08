//! GC arena bracketing on `Mrb`.
//!
//! mruby's GC does not scan the C stack; values created from C
//! frames stay alive through the fixed-size GC arena, which nothing
//! on the Rust side ever shrinks. A loop that allocates per
//! iteration therefore grows the arena until it overflows;
//! `Mrb::arena_scope` brackets such a region so its allocations are
//! released together.
//!
//! The safety contract is the spec's GC validity rule: a value
//! created inside an arena scope is not used after that scope ends,
//! and a survivor carried out through `ArenaScope::keep` counts as
//! created where the scope was opened. The type system does not
//! enforce the rule; the consumer upholds it.

use crate::{Mrb, Value};
use beni_sys as sys;

/// RAII guard over a GC arena region. Dropping it restores the
/// arena index recorded at `Mrb::arena_scope`, releasing arena
/// protection for every value created inside; `keep` instead
/// carries one surviving value out still protected.
///
/// The guard borrows the interpreter, so it stays on the thread that
/// made it:
///
/// ```compile_fail
/// fn carried<T: Send>() {}
/// carried::<beni::ArenaScope<'static>>();
/// ```
pub struct ArenaScope<'mrb> {
    mrb: &'mrb Mrb,
    idx: core::ffi::c_int,
}

impl Mrb {
    /// Open an arena scope: record the current arena index and
    /// return the guard that restores it. A raise long-jumping out
    /// of the region skips the restore along with the whole C
    /// frame — mruby unwinds the arena with its own handler.
    pub fn arena_scope(&self) -> ArenaScope<'_> {
        ArenaScope {
            mrb: self,
            // SAFETY: `self` is alive; the save helper only reads
            // the index.
            idx: unsafe { sys::mrb_gc_arena_save_func(self.as_ptr()) },
        }
    }
}

impl ArenaScope<'_> {
    /// End the scope keeping `v`: restore the arena index, then
    /// re-protect `v` so it stays alive past the scope — restoring
    /// first is what frees the slot the survivor is re-protected
    /// into.
    pub fn keep(self, v: Value) -> Value {
        let mrb = self.mrb;
        let idx = self.idx;
        // The restore below replaces the one Drop would run.
        core::mem::forget(self);
        // SAFETY: `mrb` is alive; `idx` was produced by
        // `arena_scope` against the same VM.
        unsafe { sys::mrb_gc_arena_restore_func(mrb.as_ptr(), idx) };
        // SAFETY: `v` originates from the same VM; the arena has
        // a free slot after the restore.
        unsafe { sys::mrb_gc_protect(mrb.as_ptr(), v.as_raw()) };
        v
    }
}

impl Drop for ArenaScope<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.mrb` is alive for the guard's lifetime;
        // `self.idx` was produced by `arena_scope` against the
        // same VM.
        unsafe { sys::mrb_gc_arena_restore_func(self.mrb.as_ptr(), self.idx) };
    }
}
