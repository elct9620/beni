use beni::{DataType, Mrb, RClass};
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
