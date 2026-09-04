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
//!
//! `Mrb::gc_root` is the releasable shape, and it needs the per-root
//! identity mruby's registry lacks. Each root is a slot in an array the
//! interpreter holds, so releasing one clears that slot and leaves every
//! other slot — including another root over the same value — standing.

#[cfg(mruby_linked)]
use crate::{Array, FromValue as _};
use crate::{Error, Mrb, Value};
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

    /// Root `v` until the returned guard is dropped, so it stays
    /// reachable for the collector however the arena moves and whether
    /// or not Ruby references it. Each guard owns one root: dropping it
    /// releases that root alone, leaving any other root over the same
    /// value standing. Fallible because taking a root grows the record
    /// of roots — no root is taken when it fails.
    pub fn gc_root(&self, v: Value) -> Result<GcRoot<'_>, Error> {
        #[cfg(mruby_linked)]
        {
            let table = self.root_table()?;
            let slot = table.claim(self, v)?;
            Ok(GcRoot {
                mrb: self,
                table,
                slot,
                value: v,
            })
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = v;
            crate::not_linked()
        }
    }

    /// The array holding one slot per live root, created on first use
    /// and kept reachable for the interpreter's lifetime by the global
    /// it is stored under. The name carries no `$`, so no Ruby program
    /// can reach the table by writing a global variable.
    #[cfg(mruby_linked)]
    fn root_table(&self) -> Result<RootTable, Error> {
        let slot = self.intern_static(TABLE_GLOBAL);
        if let Some(table) = Array::from_value(self.gv_get(slot)) {
            return Ok(RootTable(table));
        }

        let table = self.ary_new();
        // Index 0 is the free-list head rather than a root, so a slot
        // index is never zero and zero can mean "no free slot".
        table.push(self, Value::from_int(self, 0))?;
        self.gv_set(slot, table.as_value());
        Ok(RootTable(table))
    }
}

/// The root table: an intrusive free list over an mruby array. Slot 0
/// holds the index of the first free slot (zero when there is none),
/// and each free slot holds the index of the next one, so a released
/// slot is reused without scanning.
#[cfg(mruby_linked)]
#[derive(Clone, Copy)]
struct RootTable(Array);

#[cfg(mruby_linked)]
impl RootTable {
    /// Store `v` in a free slot, reusing a released one when the free
    /// list has any and growing the table when it does not.
    fn claim(&self, mrb: &Mrb, v: Value) -> Result<usize, Error> {
        match self.free_head(mrb) {
            0 => {
                self.0.push(mrb, v)?;
                Ok(self.0.len() - 1)
            }
            head => {
                let next = self.0.entry(head as isize);
                self.0.store(mrb, 0, next)?;
                self.0.store(mrb, head as isize, v)?;
                Ok(head)
            }
        }
    }

    /// Clear `slot` and thread it onto the front of the free list.
    fn release(&self, mrb: &Mrb, slot: usize) -> Result<(), Error> {
        let head = self.0.entry(0);
        self.0.store(mrb, slot as isize, head)?;
        self.0
            .store(mrb, 0, Value::from_int(mrb, slot as sys::mrb_int))
    }

    /// The first free slot, or zero when there is none. Slot 0 always
    /// holds the integer this type wrote there, so a read that does not
    /// answer one means the table is unusable — reported as "no free
    /// slot", which grows the table instead of reusing a wrong one.
    fn free_head(&self, mrb: &Mrb) -> usize {
        self.0
            .entry(0)
            .as_int(mrb)
            .ok()
            .and_then(|head| usize::try_from(head).ok())
            .unwrap_or(0)
    }
}

/// The global the root table is stored under. The name carries no `$`,
/// so no Ruby program can reach the table by writing a global variable.
#[cfg(mruby_linked)]
const TABLE_GLOBAL: &[u8] = b"beni_gc_roots";

/// One root over one value, released when dropped. Roots over the same
/// value are independent: each owns its own slot, so no drop releases
/// another's. The guard holds the table and the value it rooted, so
/// reading it back and releasing it need no lookup.
pub struct GcRoot<'mrb> {
    #[cfg(mruby_linked)]
    mrb: &'mrb Mrb,
    #[cfg(mruby_linked)]
    table: RootTable,
    #[cfg(mruby_linked)]
    slot: usize,
    #[cfg(mruby_linked)]
    value: Value,
    #[cfg(not(mruby_linked))]
    _mrb: core::marker::PhantomData<&'mrb Mrb>,
}

impl GcRoot<'_> {
    /// The value this root holds. Its slot changes only when the root
    /// is released, which consumes the guard, so this stays what was
    /// rooted for as long as the guard lives.
    pub fn value(&self) -> Value {
        #[cfg(mruby_linked)]
        {
            self.value
        }
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

impl Drop for GcRoot<'_> {
    fn drop(&mut self) {
        #[cfg(mruby_linked)]
        {
            // A refused release leaves the value rooted for the
            // interpreter's remaining lifetime — over-retention, never a
            // value collected while a holder still names it.
            let _ = self.table.release(self.mrb, self.slot);
        }
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use crate::{DataType, FromValue, Mrb, RClass, RString};
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

    static GUARDED_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct GuardedProbe;
    impl Drop for GuardedProbe {
        fn drop(&mut self) {
            GUARDED_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    static GUARDED_TYPE: DataType<GuardedProbe> = DataType::new(c"BeniGuardedProbe");

    static SHARED_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct SharedProbe;
    impl Drop for SharedProbe {
        fn drop(&mut self) {
            SHARED_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    static SHARED_TYPE: DataType<SharedProbe> = DataType::new(c"BeniSharedProbe");

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

    #[test]
    fn a_guard_holds_its_value_until_it_is_dropped() {
        GUARDED_DROPS.store(0, Ordering::SeqCst);
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = carrier(&mrb, c"BeniGuardedHolder");

        let root = {
            let scope = mrb.arena_scope();
            let obj = class
                .data_wrap(&mrb, GuardedProbe, &GUARDED_TYPE)
                .expect("wrapping into a marked class must succeed");
            let root = mrb.gc_root(obj).expect("taking a root must succeed");
            drop(scope);
            root
        };

        mrb.full_gc();
        assert_eq!(
            GUARDED_DROPS.load(Ordering::SeqCst),
            0,
            "the guard must hold its value across a full collection"
        );
        assert!(
            root.value().data_get(&mrb, &GUARDED_TYPE).is_some(),
            "the guard must read back the value it rooted"
        );

        drop(root);
        mrb.full_gc();
        assert_eq!(
            GUARDED_DROPS.load(Ordering::SeqCst),
            1,
            "dropping the guard must let the next collection reclaim the value"
        );
    }

    #[test]
    fn roots_over_one_value_release_independently() {
        SHARED_DROPS.store(0, Ordering::SeqCst);
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = carrier(&mrb, c"BeniSharedHolder");

        let survivor = {
            let scope = mrb.arena_scope();
            let obj = class
                .data_wrap(&mrb, SharedProbe, &SHARED_TYPE)
                .expect("wrapping into a marked class must succeed");
            let first = mrb.gc_root(obj).expect("taking a root must succeed");
            let second = mrb.gc_root(obj).expect("taking a second root must succeed");
            drop(scope);
            // Releasing one root of two is where mruby's own registry
            // would drop both: it removes by value, not by registration.
            drop(first);
            second
        };

        mrb.full_gc();
        assert_eq!(
            SHARED_DROPS.load(Ordering::SeqCst),
            0,
            "releasing one root must leave the other one holding the value"
        );

        drop(survivor);
        mrb.full_gc();
        assert_eq!(
            SHARED_DROPS.load(Ordering::SeqCst),
            1,
            "the value is reclaimed once the last root is released"
        );
    }

    #[test]
    fn a_released_slot_is_reused_by_the_next_root() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let first = mrb.str_new(b"first").as_value();
        let second = mrb.str_new(b"second").as_value();

        let root = mrb.gc_root(first).expect("taking a root must succeed");
        let slot = root.slot;
        drop(root);

        let reused = mrb.gc_root(second).expect("taking a root must succeed");
        assert_eq!(
            reused.slot, slot,
            "a released slot must be reused rather than left to grow the table"
        );
        assert_eq!(
            RString::from_value(reused.value())
                .expect("the reused slot holds the second value")
                .to_bytes(),
            b"second"
        );
    }
}
