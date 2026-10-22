//! A value read out of a container stays reachable after the container
//! lets it go: each read hands the value to Rust, and a full collection
//! afterwards must not reclaim what the caller still holds.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::value::Lazy;
use beni::{ForEach, IntoValue, Mrb, RObject, RString, ReprValue, TypedData, Value};
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
    mrb.define_class(c"BeniReachCarrier", mrb.class_object())
        .expect("defining the carrier class must succeed");
    Probe::mark_carriers(mrb).expect("marking an ordinary class must succeed");
    let class = Probe::class(mrb);
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

    let read = ary.entry::<Value>(&mrb, 0).expect("the element reads");
    ary.clear(&mrb)
        .expect("clearing an unfrozen array must succeed");
    mrb.gc_start();

    assert_held(&DROPS, "an array entry");
    assert!(!read.is_nil());
}

#[test]
fn an_instance_variable_outlives_its_overwrite() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let holder = mrb
        .class_object()
        .new_instance(&mrb, &[])
        .expect("an Object constructs without raising");
    let _root = mrb
        .gc_root(holder)
        .expect("rooting the holder must succeed");
    let holder = RObject::from_value(holder).expect("Object.new answers a plain object");
    stored_carrier(&mrb, &DROPS, |obj| {
        holder
            .ivar_set(&mrb, "@probe", obj)
            .expect("ivar_set must succeed")
    });

    let read: Value = holder
        .ivar_get(&mrb, "@probe")
        .expect("ivar_get must succeed");
    // Overwritten rather than removed: a removal answers the old value,
    // which would hold it on its own.
    holder
        .ivar_set(&mrb, "@probe", beni::value::qnil().as_value())
        .expect("ivar_set must succeed");
    mrb.gc_start();

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

    let read = mrb
        .gv_get::<_, Value>("$beni_reach")
        .expect("the global reads");
    mrb.gv_remove("$beni_reach");
    mrb.gc_start();

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
        hash.aset(&mrb, true.into_value(&mrb), obj)
            .expect("set must succeed")
    });

    let mut read = beni::value::qnil().as_value();
    hash.foreach(&mrb, |_: Value, val: Value| {
        read = val;
        Ok(ForEach::Continue)
    })
    .expect("walking an unmodified hash must succeed");
    hash.clear(&mrb)
        .expect("clearing an unfrozen hash must succeed");
    mrb.gc_start();

    assert_held(&DROPS, "a walked hash value");
    assert!(!read.is_nil());
}

#[test]
fn a_walk_err_exception_outlives_the_walk() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    hash.aset(&mrb, true.into_value(&mrb), true.into_value(&mrb))
        .expect("set must succeed");
    mrb.define_class(c"BeniReachCarrier", mrb.class_object())
        .expect("defining the carrier class must succeed");
    Probe::mark_carriers(&mrb).expect("marking an ordinary class must succeed");

    let err = hash
        .foreach(&mrb, |_: Value, _: Value| -> Result<ForEach, beni::Error> {
            Err(beni::Error::Exception(mrb.wrap(Probe(&DROPS)).as_value()))
        })
        .expect_err("the closure's Err surfaces");
    mrb.gc_start();

    assert_held(&DROPS, "a walk's Err exception");
    assert!(matches!(err, beni::Error::Exception(_)));
}

#[test]
fn a_pending_exception_outlives_its_clearing() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let runtime_error = mrb
        .exc_get("RuntimeError")
        .expect("RuntimeError is a core class");
    stored_carrier(&mrb, &DROPS, |obj| {
        let exc = runtime_error.new_str(&mrb, mrb.str_new("staged".as_bytes()));
        exc.ivar_set(&mrb, "@probe", obj)
            .expect("ivar_set must succeed");
        // SAFETY: `exc` is an exception object from this VM.
        unsafe { mrb.set_pending_exc(exc.as_value()) };
    });

    let read = mrb.pending_exc();
    mrb.clear_exc();
    mrb.gc_start();

    assert_held(&DROPS, "a pending exception");
    assert!(!read.is_nil());
}

/// Run a program that empties every Array `ObjectSpace` reaches, then
/// collect: what the typed surface keeps in Arrays of its own must
/// survive both.
fn clear_every_reachable_array(mrb: &Mrb) {
    mrb.eval::<beni::Value>("ObjectSpace.each_object(Array) { |a| a.clear unless a.frozen? }")
        .expect("walking the heap's Arrays must succeed");
    mrb.gc_start();
}

#[test]
fn a_root_outlives_a_program_clearing_every_array() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    let mut root = None;
    stored_carrier(&mrb, &DROPS, |obj| {
        root = Some(mrb.gc_root(obj).expect("taking a root must succeed"));
    });

    clear_every_reachable_array(&mrb);

    assert_held(&DROPS, "a rooted value");
    drop(root);
}

#[test]
fn a_lazy_value_outlives_a_program_clearing_every_array() {
    static HELD: Lazy<RString> = Lazy::new(|mrb| mrb.str_new(b"held"));
    let mrb = open_mrb();
    Lazy::force(&HELD, &mrb);

    clear_every_reachable_array(&mrb);

    let held = Lazy::try_get_inner(&HELD, &mrb).expect("the lazy value must still be held");
    assert_eq!(
        held.to_string(&mrb).expect("the held string renders"),
        "held"
    );
}

#[test]
fn a_walk_visits_every_pair_after_a_visit_clears_every_array() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    for (key, val) in [(1, "one"), (2, "two")] {
        hash.aset(
            &mrb,
            key.into_value(&mrb),
            mrb.str_new(val.as_bytes()).as_value(),
        )
        .expect("set must succeed");
    }

    let mut visited = Vec::new();
    hash.foreach(&mrb, |_: Value, val: RString| {
        if visited.is_empty() {
            clear_every_reachable_array(&mrb);
        }
        visited.push(val.to_string(&mrb)?);
        Ok(ForEach::Continue)
    })
    .expect("a walk whose pairs stay in place must succeed");

    assert_eq!(visited, ["one", "two"]);
}
