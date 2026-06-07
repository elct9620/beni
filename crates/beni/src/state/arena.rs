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
#[cfg(mruby_linked)]
use beni_sys as sys;

/// RAII guard over a GC arena region. Dropping it restores the
/// arena index recorded at `Mrb::arena_scope`, releasing arena
/// protection for every value created inside; `keep` instead
/// carries one surviving value out still protected.
pub struct ArenaScope<'mrb> {
    mrb: &'mrb Mrb,
    #[cfg(mruby_linked)]
    idx: core::ffi::c_int,
}

impl Mrb {
    /// Open an arena scope: record the current arena index and
    /// return the guard that restores it. A raise long-jumping out
    /// of the region skips the restore along with the whole C
    /// frame — mruby unwinds the arena with its own handler.
    pub fn arena_scope(&self) -> ArenaScope<'_> {
        #[cfg(mruby_linked)]
        {
            ArenaScope {
                mrb: self,
                // SAFETY: `self` is alive; the save helper only reads
                // the index.
                idx: unsafe { sys::mrb_gc_arena_save_func(self.as_ptr()) },
            }
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

impl ArenaScope<'_> {
    /// End the scope keeping `v`: restore the arena index, then
    /// re-protect `v` so it stays alive past the scope — restoring
    /// first is what frees the slot the survivor is re-protected
    /// into.
    pub fn keep(self, v: Value) -> Value {
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        {
            let _ = (self.mrb, v);
            crate::not_linked()
        }
    }
}

impl Drop for ArenaScope<'_> {
    fn drop(&mut self) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self.mrb` is alive for the guard's lifetime;
            // `self.idx` was produced by `arena_scope` against the
            // same VM.
            unsafe { sys::mrb_gc_arena_restore_func(self.mrb.as_ptr(), self.idx) };
        }
        #[cfg(not(mruby_linked))]
        {
            // Unreachable: `arena_scope` diverges before a guard can
            // be constructed.
            let _ = self.mrb;
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::Mrb;
    use beni_sys as sys;

    /// Current arena index — mruby's save helper only reads it.
    fn arena_index(mrb: &Mrb) -> core::ffi::c_int {
        // SAFETY: `mrb` is alive by the borrow.
        unsafe { sys::mrb_gc_arena_save_func(mrb.as_ptr()) }
    }

    #[test]
    fn scope_drop_restores_the_arena_index() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let before = arena_index(&mrb);

        {
            let _scope = mrb.arena_scope();
            for _ in 0..8 {
                let _ = mrb.str_new(b"arena growth");
            }
            assert!(
                arena_index(&mrb) > before,
                "allocations inside the scope must grow the arena"
            );
        }

        assert_eq!(
            arena_index(&mrb),
            before,
            "dropping the scope must restore the arena index"
        );
    }

    #[test]
    fn keep_restores_the_arena_and_protects_the_survivor() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let before = arena_index(&mrb);

        let scope = mrb.arena_scope();
        let _noise = mrb.str_new(b"released with the scope");
        let survivor = scope.keep(mrb.str_new(b"survivor"));

        assert_eq!(
            arena_index(&mrb),
            before + 1,
            "keep must restore the arena and re-protect exactly one slot"
        );

        // The re-protected slot keeps the survivor alive across a
        // full GC.
        // SAFETY: `mrb` is alive.
        unsafe { sys::mrb_full_gc(mrb.as_ptr()) };
        // SAFETY: `survivor` is a String-tagged value from this VM,
        // consumed before the next mruby call.
        assert_eq!(unsafe { survivor.as_bytes(&mrb) }, b"survivor");
    }

    #[test]
    fn keep_survivor_counts_as_created_in_the_opening_context() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let before = arena_index(&mrb);

        // The inner survivor lands in the outer scope's region, so
        // the outer drop releases it — the spec's "counts as created
        // where its scope was opened".
        let outer = mrb.arena_scope();
        let inner = mrb.arena_scope();
        let _survivor = inner.keep(mrb.str_new(b"outer-owned"));
        drop(outer);

        assert_eq!(
            arena_index(&mrb),
            before,
            "dropping the outer scope must release the kept survivor's slot"
        );
    }
}
