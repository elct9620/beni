use crate::support::open_mrb;
use beni::prelude::*;
use beni::{FromValue, IntoValue};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Payload with no observable drop — the value wrapped where only the
/// carrier's class matters.
#[beni::wrap(class = "BeniDataMarkedBase", name = "BeniHolder")]
struct Holder {
    tag: i32,
}

/// Drop probe with its own counter — kept distinct from `Holder` so
/// the roundtrip test's VM teardown cannot perturb the assertion.
static PROBE_DROPS: AtomicUsize = AtomicUsize::new(0);

#[beni::wrap(class = "BeniDropHolder", name = "BeniDropProbe")]
struct DropProbe;
impl Drop for DropProbe {
    fn drop(&mut self) {
        PROBE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn release_hook_drops_the_boxed_value_on_close() {
    PROBE_DROPS.store(0, Ordering::SeqCst);
    {
        let mrb = open_mrb();
        let class = mrb
            .define_class(c"BeniDropHolder", mrb.object_class())
            .expect("defining the carrier class must succeed");
        class
            .set_instance_data_tt(&mrb)
            .expect("marking an ordinary class must succeed");

        // Root the carrier so it survives until close, then let the
        // VM drop: `mrb_close` sweeps it and invokes the release hook.
        let obj = mrb.wrap_as(DropProbe, class).as_value();
        mrb.gv_set(c"$beni_data_probe", obj)
            .expect("the name interns");
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

#[beni::wrap(class = "BeniThreadHolder", name = "BeniThreadProbe")]
struct ThreadProbe;
impl Drop for ThreadProbe {
    fn drop(&mut self) {
        *RELEASED_ON
            .lock()
            .expect("the probe mutex is never poisoned") = Some(std::thread::current().id());
    }
}

#[test]
fn release_hook_runs_on_the_thread_the_interpreter_was_carried_to() {
    let mrb = open_mrb();
    let class = mrb
        .define_class(c"BeniThreadHolder", mrb.object_class())
        .expect("defining the carrier class must succeed");
    class
        .set_instance_data_tt(&mrb)
        .expect("marking an ordinary class must succeed");

    // Root the carrier so nothing collects it before the close that
    // happens on the far thread.
    let obj = mrb.wrap_as(ThreadProbe, class).as_value();
    mrb.gv_set(c"$beni_thread_probe", obj)
        .expect("the name interns");

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

#[beni::wrap(class = "BeniPanicHolder", name = "BeniPanicOnDrop")]
struct PanicOnDrop;
impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        PANIC_DROPS.fetch_add(1, Ordering::SeqCst);
        panic!("payload drop panics from the GC sweep");
    }
}

#[test]
fn release_hook_contains_a_panicking_drop_on_close() {
    PANIC_DROPS.store(0, Ordering::SeqCst);
    {
        let mrb = open_mrb();
        let class = mrb
            .define_class(c"BeniPanicHolder", mrb.object_class())
            .expect("defining the carrier class must succeed");
        class
            .set_instance_data_tt(&mrb)
            .expect("marking an ordinary class must succeed");

        // Root the carrier so `mrb_close` sweeps it and invokes the
        // release hook, which drops a payload whose `Drop` panics.
        let obj = mrb.wrap_as(PanicOnDrop, class).as_value();
        mrb.gv_set(c"$beni_data_panic", obj)
            .expect("the name interns");
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

/// `err` is the marking refusal: a `TypeError` naming the layouts that
/// accept the mark.
fn assert_mark_refused(mrb: &beni::Mrb, err: beni::Error) {
    use beni::Module;

    let message = err.message(mrb);
    match err {
        beni::Error::Exception(exc) => assert_eq!(exc.class(mrb).name(mrb), "TypeError"),
        other => panic!("the refusal must carry an exception, got {other:?}"),
    }
    assert!(
        message.contains("plain objects or data carriers"),
        "the refusal must name the layouts that accept the mark: {message}"
    );
}

#[test]
fn marking_refuses_a_singleton_class_so_no_carrier_shares_it() {
    let mrb = open_mrb();
    let cxt = beni::Ccontext::new(&mrb, c"data_singleton_test.rb")
        .expect("allocating the compile context must succeed");
    let owner = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let singleton = owner
        .singleton_class(&mrb)
        .expect("an ordinary object has a singleton class");

    let err = singleton
        .set_instance_data_tt(&mrb)
        .expect_err("a singleton class must refuse the mark");
    assert_mark_refused(&mrb, err);

    // Left unmarked, the singleton class allocates no carrier that would
    // share it with the object it belongs to.
    let wrapped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mrb.wrap_as(Holder { tag: 1 }, singleton)
    }));
    assert!(wrapped.is_err());
}

#[test]
fn marking_refuses_an_exception_class_so_its_instances_stay_exceptions() {
    let mrb = open_mrb();
    let exception = mrb
        .class_get(c"Exception")
        .expect("Exception is a core class");
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is a core class");
    let own = mrb
        .define_class(c"BeniDataRefusedError", runtime_error)
        .expect("defining the exception subclass must succeed");

    for class in [exception, runtime_error, own] {
        let err = class
            .set_instance_data_tt(&mrb)
            .expect_err("an exception class must refuse the mark");
        assert_mark_refused(&mrb, err);
        let still = <beni::ExceptionClass as beni::FromValue>::from_value(class.as_value())
            .expect("the refused class is still an exception class");
        assert!(still.exc_new(&mrb, "still an exception").is_exception());
    }
}

#[test]
fn marking_refuses_every_built_in_layout_so_its_instances_keep_it() {
    let mrb = open_mrb();
    mrb.load_string(b"class BeniDataRefusedString < String; end")
        .expect("defining the String subclass must succeed");

    for name in [
        c"String",
        c"Array",
        c"Hash",
        c"Range",
        c"Proc",
        c"Float",
        c"Integer",
        c"Symbol",
        c"Module",
        c"Class",
        c"BeniDataRefusedString",
    ] {
        let class = mrb.class_get(name).expect("the class is defined");
        let err = class
            .set_instance_data_tt(&mrb)
            .expect_err("a built-in layout must refuse the mark");
        assert_mark_refused(&mrb, err);
    }

    // Left unmarked, each class still allocates in its own layout: strings
    // and floats box, and the subclass's instances are strings its
    // inherited methods read.
    assert_eq!(
        String::from_value(mrb.str_new(b"box").as_value()),
        Some("box".to_string())
    );
    assert!(1.5f32.into_value(&mrb).is_float());
    let joined = mrb
        .load_string(b"BeniDataRefusedString.new('abc') + 'def'")
        .expect("the subclass still builds strings");
    assert_eq!(String::from_value(joined), Some("abcdef".to_string()));
}

#[test]
fn a_class_defined_from_a_marked_class_carries_data_and_accepts_the_mark() {
    let mrb = open_mrb();
    let base = mrb
        .define_class(c"BeniDataMarkedBase", mrb.object_class())
        .expect("defining the base class must succeed");
    base.set_instance_data_tt(&mrb)
        .expect("marking an ordinary class must succeed");
    let derived = mrb
        .define_class(c"BeniDataMarkedDerived", base)
        .expect("defining the subclass must succeed");

    // A class defined from a marked class is marked too.
    let obj = mrb.wrap_as(Holder { tag: 3 }, derived);
    assert_eq!(obj.get::<Holder>(&mrb).map(|h| h.tag).ok(), Some(3));
    derived
        .set_instance_data_tt(&mrb)
        .expect("a class whose instances are data carriers accepts the mark");
}

/// `err` is mruby's allocator refusal for `class`.
fn assert_allocator_undefined(mrb: &beni::Mrb, err: beni::Error, class: &str) {
    use beni::Module;

    let message = err.message(mrb);
    match err {
        beni::Error::Exception(exc) => assert_eq!(exc.class(mrb).name(mrb), "TypeError"),
        other => panic!("the refusal must carry an exception, got {other:?}"),
    }
    assert_eq!(message, format!("allocator undefined for {class}"));
}

#[test]
fn an_undefined_allocator_refuses_new_and_allocate_but_not_a_wrap_or_copy() {
    let mrb = open_mrb();
    let class = mrb
        .define_class(c"BeniDataMarkedBase", mrb.object_class())
        .expect("defining the class must succeed");
    class
        .set_instance_data_tt(&mrb)
        .expect("marking an ordinary class must succeed");
    class.undef_default_alloc_func(&mrb);

    for src in [
        &b"BeniDataMarkedBase.new"[..],
        b"BeniDataMarkedBase.allocate",
    ] {
        let err = mrb
            .load_string(src)
            .expect_err("Ruby cannot allocate the class");
        assert_allocator_undefined(&mrb, err, "BeniDataMarkedBase");
    }

    // A wrap and mruby's copies allocate without the default allocator.
    let wrapped = mrb.wrap_as(Holder { tag: 5 }, class);
    assert_eq!(wrapped.get::<Holder>(&mrb).map(|h| h.tag).ok(), Some(5));
    for copy in [c"dup", c"clone"] {
        let copied = wrapped
            .as_value()
            .funcall(&mrb, copy, &[])
            .expect("copying a carrier still allocates");
        assert!(copied.is_kind_of(&mrb, class));
    }
}

#[test]
fn only_a_class_defined_after_the_allocator_is_undefined_inherits_it() {
    let mrb = open_mrb();
    let base = mrb
        .define_class(c"BeniDataAllocBase", mrb.object_class())
        .expect("defining the base class must succeed");
    let earlier = mrb
        .define_class(c"BeniDataAllocEarlier", base)
        .expect("defining the earlier subclass must succeed");
    base.undef_default_alloc_func(&mrb);
    mrb.define_class(c"BeniDataAllocLater", base)
        .expect("defining the later subclass must succeed");

    let err = mrb
        .load_string(b"BeniDataAllocLater.new")
        .expect_err("a later subclass takes the undefined allocator");
    assert_allocator_undefined(&mrb, err, "BeniDataAllocLater");
    let instance = mrb
        .load_string(b"BeniDataAllocEarlier.new")
        .expect("an earlier subclass keeps its allocator");
    assert!(instance.is_kind_of(&mrb, earlier));
}
