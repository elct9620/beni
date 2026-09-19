//! A value read out of a container stays reachable after the container
//! lets it go: each read hands the value to Rust, and a full collection
//! afterwards must not reclaim what the caller still holds.

use crate::support::open_mrb;
use beni::{ForEach, IntoValue, Mrb, RClass, ReprValue, Value};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Payload whose `Drop` counts into the case's own counter — the only
/// way to observe from Rust that the collector reclaimed its carrier.
#[beni::wrap(class = "BeniReachCarrier", name = "BeniReachProbe")]
struct Probe(&'static AtomicUsize);
impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A carrier whose only hold, once this returns, is whatever `store`
/// put it in: the arena scope it was made in has already ended.
fn stored_carrier(mrb: &Mrb, drops: &'static AtomicUsize, store: impl FnOnce(Value)) {
    let class: RClass = mrb
        .define_class(c"BeniReachCarrier", mrb.object_class())
        .expect("defining the carrier class must succeed");
    class
        .set_instance_data_tt(mrb)
        .expect("marking an ordinary class must succeed");
    let scope = mrb.arena_scope();
    let obj = mrb.wrap_as(Probe(drops), class).as_value();
    store(obj);
    drop(scope);
}

fn assert_held(drops: &AtomicUsize, what: &str) {
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "{what} must stay reachable after its container lets it go"
    );
}

#[test]
fn an_array_entry_outlives_the_array_clearing() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    let _root = mrb
        .gc_root(ary.as_value())
        .expect("rooting the array must succeed");
    stored_carrier(&mrb, &DROPS, |obj| {
        ary.push(&mrb, obj).expect("push must succeed")
    });

    let read = ary.entry(&mrb, 0);
    ary.clear(&mrb)
        .expect("clearing an unfrozen array must succeed");
    mrb.full_gc();

    assert_held(&DROPS, "an array entry");
    assert!(!read.is_nil());
}

#[test]
fn an_instance_variable_outlives_its_overwrite() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let holder = mrb
        .object_class()
        .obj_new(&mrb, &[])
        .expect("an Object constructs without raising");
    let _root = mrb
        .gc_root(holder)
        .expect("rooting the holder must succeed");
    stored_carrier(&mrb, &DROPS, |obj| {
        holder
            .iv_set(&mrb, "@probe", obj)
            .expect("iv_set must succeed")
    });

    let read = holder.iv_get(&mrb, "@probe");
    // Overwritten rather than removed: a removal answers the old value,
    // which would hold it on its own.
    holder
        .iv_set(&mrb, "@probe", Value::nil())
        .expect("iv_set must succeed");
    mrb.full_gc();

    assert_held(&DROPS, "an instance variable's value");
    assert!(!read.is_nil());
}

#[test]
fn a_global_outlives_its_removal() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    stored_carrier(&mrb, &DROPS, |obj| {
        mrb.gv_set("$beni_reach", obj).expect("gv_set must succeed")
    });

    let read = mrb.gv_get("$beni_reach");
    mrb.gv_remove("$beni_reach");
    mrb.full_gc();

    assert_held(&DROPS, "a global's value");
    assert!(!read.is_nil());
}

#[test]
fn a_walked_hash_value_outlives_the_hash_clearing() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    let _root = mrb
        .gc_root(hash.as_value())
        .expect("rooting the hash must succeed");
    stored_carrier(&mrb, &DROPS, |obj| {
        hash.set(&mrb, true.into_value(&mrb), obj)
            .expect("set must succeed")
    });

    let mut read = Value::nil();
    hash.each(&mrb, |_, val| {
        read = val;
        ForEach::Continue
    })
    .expect("walking an unmodified hash must succeed");
    hash.clear(&mrb)
        .expect("clearing an unfrozen hash must succeed");
    mrb.full_gc();

    assert_held(&DROPS, "a walked hash value");
    assert!(!read.is_nil());
}

#[test]
fn a_pending_exception_outlives_its_clearing() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let runtime_error = mrb
        .exc_get("RuntimeError")
        .expect("RuntimeError is a core class");
    stored_carrier(&mrb, &DROPS, |obj| {
        let exc = runtime_error.exc_new(&mrb, "staged");
        exc.iv_set(&mrb, "@probe", obj)
            .expect("iv_set must succeed");
        // SAFETY: `exc` is an exception object from this VM.
        unsafe { mrb.set_pending_exc(exc) };
    });

    let read = mrb.pending_exc();
    mrb.clear_exc();
    mrb.full_gc();

    assert_held(&DROPS, "a pending exception");
    assert!(!read.is_nil());
}
