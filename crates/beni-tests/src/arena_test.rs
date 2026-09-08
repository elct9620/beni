use beni::{FromValue, Mrb, RString};

/// Current arena index — mruby's save helper only reads it.
fn arena_index(mrb: &Mrb) -> core::ffi::c_int {
    // SAFETY: `mrb` is alive by the borrow.
    unsafe { beni::sys::mrb_gc_arena_save_func(mrb.as_ptr()) }
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
    let survivor = scope.keep(mrb.str_new(b"survivor").as_value());

    assert_eq!(
        arena_index(&mrb),
        before + 1,
        "keep must restore the arena and re-protect exactly one slot"
    );

    // The re-protected slot keeps the survivor alive across a
    // full GC.
    // SAFETY: `mrb` is alive.
    unsafe { beni::sys::mrb_full_gc(mrb.as_ptr()) };
    // The survivor's bytes are still readable after the GC.
    let survived = RString::from_value(survivor).expect("the survivor is String-tagged");
    assert_eq!(survived.to_bytes(), b"survivor");
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
    let _survivor = inner.keep(mrb.str_new(b"outer-owned").as_value());
    drop(outer);

    assert_eq!(
        arena_index(&mrb),
        before,
        "dropping the outer scope must release the kept survivor's slot"
    );
}
