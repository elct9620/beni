//! GC roots on `Mrb`.
//!
//! The arena reaches only as far as the C frame that opened it, so a
//! value a Rust caller holds past that frame needs a root — a hold that
//! keeps it reachable for the collector independently of the arena and
//! of any Ruby reference to it.
//!
//! `Mrb::gc_register_forever` is the shape whose root is never released.
//! Never releasing is what makes it safe: mruby's registry is keyed by
//! value rather than by registration, so a removal drops every root over
//! that value, and a shape that never removes cannot drop another
//! holder's.

use crate::{Mrb, Value};
#[cfg(mruby_linked)]
use beni_sys as sys;

impl Mrb {
    /// Root `v` for this interpreter's remaining lifetime, so it stays
    /// reachable for the collector however the arena moves and whether
    /// or not Ruby references it. The root is never released, so the
    /// value is never reclaimed — the shape for a value an embedder
    /// holds as long as the VM itself, such as a cached class handle.
    /// Rooting an immediate value is a no-op, immediates being values
    /// the collector never reclaims.
    pub fn gc_register_forever(&self, v: Value) {
        #[cfg(mruby_linked)]
        {
            // SAFETY: `self` is alive by the `&self` borrow and `v`
            // originates from the same VM; the registry only records the
            // value, and this shape never removes it again.
            unsafe { sys::mrb_gc_register(self.as_ptr(), v.as_raw()) };
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = v;
            crate::not_linked()
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{DataType, Mrb, RClass};
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Payload whose `Drop` records that the collector reclaimed its
    /// carrier — the only way to observe a collection from Rust. Each
    /// case carries its own probe and counter so one test's VM teardown
    /// cannot perturb another's assertion.
    static ROOTED_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct RootedProbe;
    impl Drop for RootedProbe {
        fn drop(&mut self) {
            ROOTED_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    static ROOTED_TYPE: DataType<RootedProbe> = DataType::new(c"BeniRootedProbe");

    static LOOSE_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct LooseProbe;
    impl Drop for LooseProbe {
        fn drop(&mut self) {
            LOOSE_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    static LOOSE_TYPE: DataType<LooseProbe> = DataType::new(c"BeniLooseProbe");

    /// A class whose instances carry a data payload, defined under a
    /// name of its own so the probes cannot collide.
    fn carrier(mrb: &Mrb, name: &'static core::ffi::CStr) -> RClass {
        let class = mrb
            .define_class(name, mrb.object_class())
            .expect("defining the carrier class must succeed");
        class.set_instance_data_tt(mrb);
        class
    }

    #[test]
    fn a_registered_value_survives_collection() {
        ROOTED_DROPS.store(0, Ordering::SeqCst);
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = carrier(&mrb, c"BeniRootedHolder");

        {
            // The wrap leaves the carrier in the arena, so the scope's
            // end is what leaves the root as its only hold.
            let scope = mrb.arena_scope();
            let obj = class
                .data_wrap(&mrb, RootedProbe, &ROOTED_TYPE)
                .expect("wrapping into a marked class must succeed");
            mrb.gc_register_forever(obj);
            drop(scope);
        }
        for _ in 0..64 {
            let _ = mrb.str_new(b"garbage");
        }

        mrb.full_gc();

        assert_eq!(
            ROOTED_DROPS.load(Ordering::SeqCst),
            0,
            "a registered value must survive a full collection"
        );
    }

    /// The counterpart that proves the assertion above is watching
    /// something: the same carrier without the root is reclaimed by the
    /// same collection.
    #[test]
    fn an_unregistered_value_is_reclaimed_by_the_same_collection() {
        LOOSE_DROPS.store(0, Ordering::SeqCst);
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = carrier(&mrb, c"BeniLooseHolder");

        {
            let scope = mrb.arena_scope();
            let _obj = class
                .data_wrap(&mrb, LooseProbe, &LOOSE_TYPE)
                .expect("wrapping into a marked class must succeed");
            drop(scope);
        }

        mrb.full_gc();

        assert_eq!(
            LOOSE_DROPS.load(Ordering::SeqCst),
            1,
            "an unrooted carrier must be reclaimed, or the rooted case proves nothing"
        );
    }
}
