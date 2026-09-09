use crate::support::open_mrb;

#[test]
fn open_boots_and_closes_a_live_interpreter() {
    // With the vendored archive linked, `open` boots a real
    // interpreter through `mrb_open`; the drop runs `mrb_close`.
    // This is the host-native smoke test of the whole link graph
    // (bindings + trampolines + the archive).
    let mrb = open_mrb();
    drop(mrb);
}

#[test]
fn an_interpreter_is_carried_between_threads() {
    use beni::FromValue;

    // One interpreter, reached from two threads in turn: it opens
    // here, evaluates on the thread it is handed to, comes back,
    // and the state the far thread wrote is what this one reads.
    let mrb = open_mrb();
    let here = mrb.load_string(b"1 + 2").expect("evaluates on this thread");
    assert_eq!(i32::from_value(here), Some(3));

    let mrb = std::thread::spawn(move || {
        let there = mrb
            .load_string(b"$carried = 40 + 2")
            .expect("evaluates on the thread it was handed to");
        assert_eq!(i32::from_value(there), Some(42));
        mrb
    })
    .join()
    .expect("the thread carrying the interpreter ran to completion");

    let back = mrb.load_string(b"$carried").expect("evaluates here again");
    assert_eq!(
        i32::from_value(back),
        Some(42),
        "the interpreter carried its own heap across both handovers"
    );
}

#[test]
fn gc_triggers_keep_a_reachable_value_valid() {
    use beni::{FromValue, RString};

    let mrb = open_mrb();

    // Anchor one survivor through a global so it stays reachable
    // across collection; pile up unreachable garbage around it.
    mrb.gv_set(
        mrb.intern_static(b"$survivor"),
        mrb.str_new(b"survivor").as_value(),
    );
    for _ in 0..64 {
        let _ = mrb.str_new(b"garbage");
    }

    // Both triggers run without raising; the VM survives.
    mrb.full_gc();
    mrb.incremental_gc();

    // The reachable survivor is still a valid String afterwards.
    let kept = mrb.gv_get(mrb.intern_static(b"$survivor"));
    let kept = RString::from_value(kept).expect("the survivor is String-tagged");
    assert_eq!(kept.to_bytes(), b"survivor");
}

/// A buffer generous enough to carve several heap pages from; the
/// page struct is private to mruby's gc.c, so the size is chosen to
/// clear it rather than computed from it.
const REGION_BYTES: usize = 512 * 1024;

#[test]
fn a_generous_region_yields_pages_the_vm_then_allocates_into() {
    let mrb = open_mrb();
    let buf = Box::leak(vec![0u8; REGION_BYTES].into_boxed_slice());

    let pages = mrb.gc_add_region(buf);
    assert!(pages > 0, "a {REGION_BYTES}-byte buffer must yield pages");

    // The VM keeps working with the region linked in, and closing it
    // releases only the descriptors — never the caller's buffer.
    for _ in 0..2048 {
        let _ = mrb.str_new(b"allocated after the region was added");
    }
    mrb.full_gc();
    drop(mrb);
}

#[test]
fn a_region_too_small_for_one_page_yields_none() {
    let mrb = open_mrb();
    let buf = Box::leak(vec![0u8; 16].into_boxed_slice());

    assert_eq!(
        mrb.gc_add_region(buf),
        0,
        "a buffer too small to hold one page must add none"
    );

    // Adding nothing leaves the interpreter allocating as before.
    let kept = mrb.str_new(b"still allocating");
    assert_eq!(kept.to_bytes(), b"still allocating");
}

#[test]
fn pending_exc_reads_the_slot_and_clear_exc_empties_it() {
    let mrb = open_mrb();

    // A fresh VM has no pending exception.
    assert!(mrb.pending_exc().is_nil());

    // Install a real exception object; the read hands it back
    // without clearing the slot.
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is a core class");
    let exc = runtime_error.exc_new(&mrb, "installed by the test");
    // SAFETY: `exc` is the exception object `exc_new` just built
    // on this VM.
    unsafe { mrb.set_pending_exc(exc) };
    assert_eq!(mrb.pending_exc().classname(&mrb), "RuntimeError");
    assert_eq!(mrb.pending_exc().classname(&mrb), "RuntimeError");

    // Clearing empties the slot; clearing again is idempotent.
    mrb.clear_exc();
    assert!(mrb.pending_exc().is_nil());
    mrb.clear_exc();
    assert!(mrb.pending_exc().is_nil());
}
