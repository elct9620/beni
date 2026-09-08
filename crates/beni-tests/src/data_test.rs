use beni::DataType;
use beni::Mrb;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Payload with no observable drop — exercises wrap / get / type
/// checking without touching the drop-probe counter.
struct Holder {
    tag: i32,
}

static HOLDER_TYPE: DataType<Holder> = DataType::new(c"BeniHolder");
static OTHER_TYPE: DataType<Holder> = DataType::new(c"BeniOtherHolder");

#[test]
fn data_wrap_roundtrips_and_get_is_type_checked() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb
        .define_class(c"BeniDataHolder", mrb.object_class())
        .expect("defining the carrier class must succeed");
    class.set_instance_data_tt(&mrb);

    let obj = class
        .data_wrap(&mrb, Holder { tag: 7 }, &HOLDER_TYPE)
        .expect("wrapping into a marked class must succeed");
    assert!(obj.is_data(), "a wrapped carrier reports the data tag");

    let got = obj
        .data_get(&mrb, &HOLDER_TYPE)
        .expect("the matching data type extracts");
    assert_eq!(got.tag, 7);

    // A different descriptor (distinct `mrb_data_type` identity) and
    // a non-data value both reject instead of misreading the pointer.
    assert!(
        obj.data_get(&mrb, &OTHER_TYPE).is_none(),
        "a different data type must not extract"
    );
    assert!(
        mrb.str_new(b"x")
            .as_value()
            .data_get(&mrb, &HOLDER_TYPE)
            .is_none(),
        "a non-data value must not extract"
    );
}

/// Drop probe with its own counter for the failed-wrap path — kept
/// distinct from the close-time probes so the unmarked-class test's
/// reclaim assertion cannot be perturbed by another test's teardown.
static UNMARKED_DROPS: AtomicUsize = AtomicUsize::new(0);

struct UnmarkedProbe;
impl Drop for UnmarkedProbe {
    fn drop(&mut self) {
        UNMARKED_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

static UNMARKED_TYPE: DataType<UnmarkedProbe> = DataType::new(c"BeniUnmarkedProbe");

#[test]
fn data_wrap_into_an_unmarked_class_errs_and_reclaims_the_box() {
    UNMARKED_DROPS.store(0, Ordering::SeqCst);
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    // A class that was never marked through set_instance_data_tt: its
    // instances do not allocate as data carriers, so the allocation
    // raises a TypeError instead of producing a carrier.
    let class = mrb
        .define_class(c"BeniUnmarkedHolder", mrb.object_class())
        .expect("defining the class must succeed");

    let err = class
        .data_wrap(&mrb, UnmarkedProbe, &UNMARKED_TYPE)
        .expect_err("wrapping into an unmarked class must surface an Err");

    // The raise is mruby's allocation TypeError, surfaced rather than
    // unwound across the boundary.
    let exc = match err {
        beni::Error::Exception(v) => v,
        beni::Error::Panic(_) => unreachable!("the allocation raise is a Ruby exception"),
    };
    assert_eq!(exc.classname(&mrb), "TypeError");

    // The box handed to the failed allocation was reclaimed exactly
    // once — proving the orphaned payload is freed, not leaked.
    assert_eq!(
        UNMARKED_DROPS.load(Ordering::SeqCst),
        1,
        "the unwrapped payload must be dropped once on the failed-wrap path"
    );

    // The VM survives the protected raise and stays usable.
    let alive = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive the failed wrap");
    assert_eq!(alive.to_string(&mrb), "alive");
}

#[test]
fn data_reinit_installs_into_a_bare_carrier() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb
        .define_class(c"BeniReinitHolder", mrb.object_class())
        .expect("defining the carrier class must succeed");
    class.set_instance_data_tt(&mrb);

    // A bare carrier — allocated as CDATA with no payload yet, the
    // shape mruby's dup/clone hands to initialize_copy.
    let obj = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    assert!(obj.is_data(), "the fresh instance is a data carrier");
    assert!(
        obj.data_get(&mrb, &HOLDER_TYPE).is_none(),
        "a bare carrier holds no payload before re-init"
    );

    obj.data_reinit(&mrb, Holder { tag: 99 }, &HOLDER_TYPE);
    let got = obj
        .data_get(&mrb, &HOLDER_TYPE)
        .expect("the re-installed payload extracts under its type");
    assert_eq!(got.tag, 99);
}

#[test]
fn data_reinit_on_a_non_carrier_is_a_safe_noop() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A non-CDATA value: the install must do nothing rather than
    // reinterpret the value's bytes as a data carrier.
    let string = mrb.str_new(b"intact");
    let value = string.as_value();
    assert!(!value.is_data(), "a string is not a data carrier");

    value.data_reinit(&mrb, Holder { tag: 1 }, &HOLDER_TYPE);

    assert!(
        !value.is_data(),
        "the value stays a non-carrier after the no-op install"
    );
    assert!(
        value.data_get(&mrb, &HOLDER_TYPE).is_none(),
        "no payload is installed into a non-carrier"
    );
    assert_eq!(
        string.to_bytes(),
        b"intact",
        "the value's own representation is untouched"
    );
}

/// Drop probe with its own counter — kept distinct from `Holder` so
/// the roundtrip test's VM teardown cannot perturb the assertion.
static PROBE_DROPS: AtomicUsize = AtomicUsize::new(0);

struct DropProbe;
impl Drop for DropProbe {
    fn drop(&mut self) {
        PROBE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

static PROBE_TYPE: DataType<DropProbe> = DataType::new(c"BeniDropProbe");

#[test]
fn release_hook_drops_the_boxed_value_on_close() {
    PROBE_DROPS.store(0, Ordering::SeqCst);
    {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb
            .define_class(c"BeniDropHolder", mrb.object_class())
            .expect("defining the carrier class must succeed");
        class.set_instance_data_tt(&mrb);

        // Root the carrier so it survives until close, then let the
        // VM drop: `mrb_close` sweeps it and invokes the release hook.
        let obj = class
            .data_wrap(&mrb, DropProbe, &PROBE_TYPE)
            .expect("wrapping into a marked class must succeed");
        let slot = mrb.intern_cstr(c"$beni_data_probe");
        mrb.gv_set(slot, obj);
    }
    assert_eq!(
        PROBE_DROPS.load(Ordering::SeqCst),
        1,
        "the release hook must drop the boxed payload exactly once"
    );
}

/// Records which thread released it, so a close on a thread the
/// interpreter was handed to can be told from a close at home.
static RELEASED_ON: std::sync::Mutex<Option<std::thread::ThreadId>> = std::sync::Mutex::new(None);

struct ThreadProbe;
impl Drop for ThreadProbe {
    fn drop(&mut self) {
        *RELEASED_ON
            .lock()
            .expect("the probe mutex is never poisoned") = Some(std::thread::current().id());
    }
}

static THREAD_PROBE_TYPE: DataType<ThreadProbe> = DataType::new(c"BeniThreadProbe");

#[test]
fn release_hook_runs_on_the_thread_the_interpreter_was_carried_to() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb
        .define_class(c"BeniThreadHolder", mrb.object_class())
        .expect("defining the carrier class must succeed");
    class.set_instance_data_tt(&mrb);

    // Root the carrier so nothing collects it before the close that
    // happens on the far thread.
    let obj = class
        .data_wrap(&mrb, ThreadProbe, &THREAD_PROBE_TYPE)
        .expect("wrapping into a marked class must succeed");
    let slot = mrb.intern_cstr(c"$beni_thread_probe");
    mrb.gv_set(slot, obj);

    let carrier = std::thread::spawn(move || {
        let id = std::thread::current().id();
        drop(mrb);
        id
    })
    .join()
    .expect("the thread carrying the interpreter ran to completion");

    assert_ne!(
        carrier,
        std::thread::current().id(),
        "the close must have happened away from this thread"
    );
    assert_eq!(
        *RELEASED_ON
            .lock()
            .expect("the probe mutex is never poisoned"),
        Some(carrier),
        "the payload is released wherever its interpreter is reached from"
    );
}

/// Records that its `Drop` ran, then panics — a consumer payload
/// whose destructor unwinds while the GC sweeps it from a C frame.
static PANIC_DROPS: AtomicUsize = AtomicUsize::new(0);

struct PanicOnDrop;
impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        PANIC_DROPS.fetch_add(1, Ordering::SeqCst);
        panic!("payload drop panics from the GC sweep");
    }
}

static PANIC_TYPE: DataType<PanicOnDrop> = DataType::new(c"BeniPanicOnDrop");

#[test]
fn release_hook_contains_a_panicking_drop_on_close() {
    PANIC_DROPS.store(0, Ordering::SeqCst);
    {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
        let class = mrb
            .define_class(c"BeniPanicHolder", mrb.object_class())
            .expect("defining the carrier class must succeed");
        class.set_instance_data_tt(&mrb);

        // Root the carrier so `mrb_close` sweeps it and invokes the
        // release hook, which drops a payload whose `Drop` panics.
        let obj = class
            .data_wrap(&mrb, PanicOnDrop, &PANIC_TYPE)
            .expect("wrapping into a marked class must succeed");
        let slot = mrb.intern_cstr(c"$beni_data_panic");
        mrb.gv_set(slot, obj);
    }
    // Reaching here proves the panic was contained at the hook: had
    // it unwound across mruby's C sweep frame the process would have
    // aborted instead. The drop still ran.
    assert_eq!(
        PANIC_DROPS.load(Ordering::SeqCst),
        1,
        "the release hook must run the payload's drop even when it panics"
    );
}
