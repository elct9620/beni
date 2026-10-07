//! `Lazy`: a value a `static` names, computed once in each interpreter and
//! held for that interpreter's lifetime.

use crate::support::open_mrb;
use beni::value::Lazy;
use beni::{RString, ReprValue, TypedData};
use core::sync::atomic::{AtomicUsize, Ordering};

static COMPUTED: AtomicUsize = AtomicUsize::new(0);
static COUNTED: Lazy<RString> = Lazy::new(|mrb| {
    COMPUTED.fetch_add(1, Ordering::SeqCst);
    mrb.str_new(b"counted")
});

#[test]
fn each_interpreter_computes_its_value_once() {
    let first = open_mrb();
    let second = open_mrb();
    let before = COMPUTED.load(Ordering::SeqCst);

    let a = first.get_inner(&COUNTED);
    let again = first.get_inner(&COUNTED);
    let b = second.get_inner(&COUNTED);

    assert_eq!(COMPUTED.load(Ordering::SeqCst) - before, 2);
    assert!(a.as_value().is_equal(&first, again.as_value()));
    assert_eq!(b.to_string(&second).expect("UTF-8"), "counted");
}

static FORCED: Lazy<RString> = Lazy::new(|mrb| mrb.str_new(b"forced"));

#[test]
fn try_get_inner_reads_only_what_force_held() {
    let mrb = open_mrb();

    assert!(Lazy::try_get_inner(&FORCED, &mrb).is_none());
    Lazy::force(&FORCED, &mrb);

    let held = Lazy::try_get_inner(&FORCED, &mrb).expect("force held the value");
    assert_eq!(held.to_string(&mrb).expect("UTF-8"), "forced");
}

#[beni::wrap(class = "BeniLazyCarrier", name = "BeniLazyProbe")]
struct Probe(&'static AtomicUsize);
impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

static DROPS: AtomicUsize = AtomicUsize::new(0);
static PROBE: Lazy<beni::RTypedData> = Lazy::new(|mrb| mrb.wrap(Probe(&DROPS)));

#[test]
fn a_held_value_outlives_the_scope_that_computed_it() {
    let mrb = open_mrb();
    mrb.define_class(c"BeniLazyCarrier", mrb.object_class())
        .expect("defining the carrier class must succeed");
    Probe::mark_carriers(&mrb).expect("marking an ordinary class must succeed");

    let scope = mrb.arena_scope();
    Lazy::force(&PROBE, &mrb);
    drop(scope);
    mrb.load_string(b"$beni_lazy = nil; beni_lazy = nil")
        .expect("assigning those names succeeds");
    mrb.full_gc();

    assert_eq!(DROPS.load(Ordering::SeqCst), 0, "the interpreter holds it");
    assert!(Lazy::try_get_inner(&PROBE, &mrb).is_some());
}

#[test]
fn every_record_beni_keeps_shares_one_global() {
    static ROOTED_DROPS: AtomicUsize = AtomicUsize::new(0);
    let mrb = open_mrb();
    mrb.define_class(c"BeniLazyCarrier", mrb.object_class())
        .expect("defining the carrier class must succeed");
    Probe::mark_carriers(&mrb).expect("marking an ordinary class must succeed");
    let wrapped = mrb.wrap(Probe(&ROOTED_DROPS));
    let _root = mrb
        .gc_root(wrapped.as_value())
        .expect("rooting the carrier must succeed");

    assert!(mrb.check_id(b"beni_lazy").is_some());
    for name in [
        b"beni_gc_roots".as_slice(),
        b"beni_carriers",
        b"beni_inline",
        b"beni_inline_classes",
    ] {
        assert!(
            mrb.check_id(name).is_none(),
            "{} names no global",
            String::from_utf8_lossy(name)
        );
    }
}
