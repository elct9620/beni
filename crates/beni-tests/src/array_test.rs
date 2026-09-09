use crate::support::open_mrb;
use beni::Error;

#[test]
fn push_and_entry_roundtrip_through_a_live_array() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();

    ary.push(&mrb, mrb.str_new(b"first").as_value())
        .expect("push to a fresh array succeeds");
    ary.push(&mrb, mrb.str_new(b"second").as_value())
        .expect("push to a fresh array succeeds");

    assert_eq!(ary.entry(0).to_string(&mrb), "first");
    assert_eq!(ary.entry(-1).to_string(&mrb), "second");
}

#[test]
fn entry_is_nil_out_of_range_in_both_directions() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();

    ary.push(&mrb, mrb.str_new(b"only").as_value())
        .expect("push to a fresh array succeeds");

    assert!(ary.entry(1).is_nil());
    assert!(ary.entry(-2).is_nil());
    // An index beyond the archive's `mrb_int` width is out of
    // range by definition — same nil contract, no truncation.
    assert!(ary.entry(isize::MAX).is_nil());
    assert!(ary.entry(isize::MIN).is_nil());
}

#[test]
fn store_writes_grows_and_counts_from_the_tail() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();

    // Writing past the end grows the array, filling the gap with nil.
    ary.store(&mrb, 2, mrb.str_new(b"two").as_value())
        .expect("an in-range store succeeds");
    assert_eq!(ary.len(), 3);
    assert!(ary.entry(0).is_nil());
    assert!(ary.entry(1).is_nil());
    assert_eq!(ary.entry(2).to_string(&mrb), "two");

    // A negative index counts from the tail.
    ary.store(&mrb, -1, mrb.str_new(b"last").as_value())
        .expect("a negative in-range store succeeds");
    assert_eq!(ary.entry(2).to_string(&mrb), "last");
}

#[test]
fn store_out_of_range_index_surfaces_err() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, mrb.str_new(b"only").as_value())
        .expect("push to a fresh array succeeds");

    // A negative index reaching past the beginning raises IndexError.
    assert!(matches!(
        ary.store(&mrb, -5, mrb.str_new(b"x").as_value()),
        Err(Error::Exception(_))
    ));
    // An index beyond the archive's `mrb_int` width saturates so
    // mruby's own range check rejects it as too large, rather than a
    // truncated index hitting the wrong slot.
    assert!(matches!(
        ary.store(&mrb, isize::MAX, mrb.str_new(b"x").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn len_and_is_empty_track_the_element_count() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();

    assert_eq!(ary.len(), 0);
    assert!(ary.is_empty());

    ary.push(&mrb, mrb.str_new(b"a").as_value())
        .expect("push to a fresh array succeeds");
    ary.push(&mrb, mrb.str_new(b"b").as_value())
        .expect("push to a fresh array succeeds");

    assert_eq!(ary.len(), 2);
    assert!(!ary.is_empty());
}

#[test]
fn push_surfaces_frozen_receiver_as_err() {
    use beni::{Array, Ccontext, FromValue};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"frozen_ary.rb").expect("allocating the context must succeed");

    // A frozen Array still carries the Array tag, so the downcast
    // holds, but pushing to it raises FrozenError — which protect
    // catches into Err rather than long-jumping.
    let frozen = Array::from_value(
        cxt.load_nstring(b"[].freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Array literal is Array-tagged");
    assert!(matches!(
        frozen.push(&mrb, mrb.str_new(b"x").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn pop_and_shift_remove_from_each_end() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, mrb.str_new(b"a").as_value())
        .expect("push succeeds");
    ary.push(&mrb, mrb.str_new(b"b").as_value())
        .expect("push succeeds");

    assert_eq!(ary.pop(&mrb).expect("pop succeeds").to_string(&mrb), "b");
    assert_eq!(
        ary.shift(&mrb).expect("shift succeeds").to_string(&mrb),
        "a"
    );
    // Draining past empty yields nil, not an error.
    assert!(ary.pop(&mrb).expect("pop on empty succeeds").is_nil());
    assert!(ary.shift(&mrb).expect("shift on empty succeeds").is_nil());
}

#[test]
fn unshift_prepends_and_concat_extends() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, mrb.str_new(b"mid").as_value())
        .expect("push succeeds");
    ary.unshift(&mrb, mrb.str_new(b"head").as_value())
        .expect("unshift succeeds");

    let tail = mrb.ary_new();
    tail.push(&mrb, mrb.str_new(b"tail").as_value())
        .expect("push succeeds");
    ary.concat(&mrb, tail).expect("concat succeeds");

    assert_eq!(ary.entry(0).to_string(&mrb), "head");
    assert_eq!(ary.entry(1).to_string(&mrb), "mid");
    assert_eq!(ary.entry(2).to_string(&mrb), "tail");
}

#[test]
fn clear_empties_and_dup_copies_independently() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, mrb.str_new(b"x").as_value())
        .expect("push succeeds");

    let copy = ary.dup(&mrb);
    ary.clear(&mrb).expect("clear succeeds");

    // clear emptied the original; dup is an independent array.
    assert!(ary.is_empty());
    assert_eq!(copy.len(), 1);
    assert_eq!(copy.entry(0).to_string(&mrb), "x");
}

#[test]
fn replace_swaps_the_whole_contents_in_place() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, beni::Value::from_int(&mrb, 1))
        .expect("push succeeds");
    ary.push(&mrb, beni::Value::from_int(&mrb, 2))
        .expect("push succeeds");

    let other = mrb.ary_new();
    for n in [3, 4, 5] {
        other
            .push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }

    ary.replace(&mrb, other).expect("replace succeeds");

    // The receiver now holds a copy of other's elements, in place.
    assert_eq!(ary.len(), 3);
    assert_eq!(ary.entry(0).to_string(&mrb), "3");
    assert_eq!(ary.entry(1).to_string(&mrb), "4");
    assert_eq!(ary.entry(2).to_string(&mrb), "5");
}

#[test]
fn resize_grows_with_nil_and_truncates() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, mrb.str_new(b"a").as_value())
        .expect("push succeeds");

    // Growing past the current length fills the new slots with nil.
    ary.resize(&mrb, 3).expect("grow succeeds");
    assert_eq!(ary.len(), 3);
    assert_eq!(ary.entry(0).to_string(&mrb), "a");
    assert!(ary.entry(1).is_nil());
    assert!(ary.entry(2).is_nil());

    // Truncating drops the tail.
    ary.resize(&mrb, 1).expect("truncate succeeds");
    assert_eq!(ary.len(), 1);
    assert_eq!(ary.entry(0).to_string(&mrb), "a");
}

#[test]
fn splice_inserts_replaces_and_deletes_in_place() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    for n in [1, 2, 3] {
        ary.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }

    // A zero-length splice inserts the replacement's elements without
    // removing any: [1,2,3] -> [1,10,11,2,3].
    let ins = mrb.ary_new();
    for n in [10, 11] {
        ins.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }
    ary.splice(&mrb, 1, 0, ins.as_value())
        .expect("a zero-length splice succeeds");
    assert_eq!(ary.len(), 5);
    assert_eq!(ary.entry(1).to_string(&mrb), "10");
    assert_eq!(ary.entry(2).to_string(&mrb), "11");

    // A non-array replacement is inserted as the single element it is,
    // replacing the run in place: [1,10,11,2,3] -> [1,10,99,2,3].
    ary.splice(&mrb, 2, 1, beni::Value::from_int(&mrb, 99))
        .expect("an in-place single-element replace succeeds");
    assert_eq!(ary.len(), 5);
    assert_eq!(ary.entry(2).to_string(&mrb), "99");

    // Replacing with fewer elements than removed shrinks the array;
    // an empty replacement deletes outright: removing the two slots at
    // index 2 leaves [1,10,3].
    ary.splice(&mrb, 2, 2, mrb.ary_new().as_value())
        .expect("a shrinking delete-and-replace succeeds");
    assert_eq!(ary.len(), 3);
    assert_eq!(ary.entry(0).to_string(&mrb), "1");
    assert_eq!(ary.entry(1).to_string(&mrb), "10");
    assert_eq!(ary.entry(2).to_string(&mrb), "3");

    // The return value is the receiver itself.
    let returned = ary
        .splice(&mrb, 0, 0, mrb.ary_new().as_value())
        .expect("a no-op splice succeeds");
    assert_eq!(returned.to_string(&mrb), ary.as_value().to_string(&mrb));
}

#[test]
fn splice_surfaces_raising_edges_as_err() {
    use beni::{Array, Ccontext, FromValue};

    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, beni::Value::from_int(&mrb, 1))
        .expect("push succeeds");

    // A head reaching past the beginning raises IndexError.
    assert!(matches!(
        ary.splice(&mrb, -5, 0, mrb.ary_new().as_value()),
        Err(Error::Exception(_))
    ));
    // A negative length raises IndexError.
    assert!(matches!(
        ary.splice(&mrb, 0, -1, mrb.ary_new().as_value()),
        Err(Error::Exception(_))
    ));
    // A head beyond the archive's mrb_int width saturates so mruby's
    // own range check rejects it as out of array, rather than a
    // truncated index hitting the wrong slot.
    assert!(matches!(
        ary.splice(&mrb, i64::MAX, 0, mrb.ary_new().as_value()),
        Err(Error::Exception(_))
    ));

    // A frozen receiver raises FrozenError before any work — splice
    // routes through mrb_ary_modify like the other mutators.
    let cxt =
        Ccontext::new(&mrb, c"frozen_splice.rb").expect("allocating the context must succeed");
    let frozen = Array::from_value(
        cxt.load_nstring(b"[1].freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Array literal is Array-tagged");
    assert!(matches!(
        frozen.splice(&mrb, 0, 1, mrb.ary_new().as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn join_renders_elements_with_a_separator() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, beni::Value::from_int(&mrb, 1))
        .expect("push succeeds");
    ary.push(&mrb, beni::Value::from_int(&mrb, 2))
        .expect("push succeeds");
    ary.push(&mrb, beni::Value::from_int(&mrb, 3))
        .expect("push succeeds");

    // Each element's to_s runs and the separator sits between adjacent
    // renderings.
    let joined = ary
        .join(&mrb, Some(mrb.str_new(b",")))
        .expect("join with a separator succeeds");
    assert_eq!(joined.to_bytes(), b"1,2,3".to_vec());

    // A None separator concatenates the renderings with nothing between.
    let glued = ary
        .join(&mrb, None)
        .expect("join without a separator succeeds");
    assert_eq!(glued.to_bytes(), b"123".to_vec());
}

#[test]
fn join_surfaces_a_raising_element_to_s_as_err() {
    use beni::{Array, Ccontext, FromValue};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"join_raise.rb").expect("allocating the context must succeed");

    // An element whose to_s raises long-jumps out of mrb_ary_join;
    // protect catches it into Err rather than unwinding across FFI.
    let ary = Array::from_value(
        cxt.load_nstring(b"o = Object.new; def o.to_s; raise 'boom'; end; [o]")
            .expect("the test source must compile and run"),
    )
    .expect("an Array literal is Array-tagged");
    assert!(matches!(ary.join(&mrb, None), Err(Error::Exception(_))));
}

#[test]
fn pop_surfaces_frozen_receiver_as_err() {
    use beni::{Array, Ccontext, FromValue};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"frozen_pop.rb").expect("allocating the context must succeed");

    // pop checks frozen state before touching the elements, so even a
    // populated frozen array surfaces FrozenError as Err.
    let frozen = Array::from_value(
        cxt.load_nstring(b"[1].freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Array literal is Array-tagged");
    assert!(matches!(frozen.pop(&mrb), Err(Error::Exception(_))));
}

#[test]
fn remaining_mutators_surface_frozen_receiver_as_err() {
    use beni::{Array, Ccontext, FromValue};

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"frozen_ary_mut.rb").expect("allocating the context must succeed");

    // Every mutator routes through mrb_ary_modify, which raises
    // FrozenError on a frozen receiver — protect catches each into Err.
    // push and pop are pinned separately; this covers the rest.
    let frozen = Array::from_value(
        cxt.load_nstring(b"[1].freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Array literal is Array-tagged");
    let other = mrb.ary_new();

    assert!(matches!(
        frozen.unshift(&mrb, mrb.str_new(b"x").as_value()),
        Err(Error::Exception(_))
    ));
    assert!(matches!(
        frozen.concat(&mrb, other),
        Err(Error::Exception(_))
    ));
    assert!(matches!(
        frozen.replace(&mrb, other),
        Err(Error::Exception(_))
    ));
    assert!(matches!(frozen.shift(&mrb), Err(Error::Exception(_))));
    assert!(matches!(frozen.clear(&mrb), Err(Error::Exception(_))));
    assert!(matches!(frozen.resize(&mrb, 5), Err(Error::Exception(_))));
    // An in-range indexed write reaches the frozen check too, not just
    // the out-of-range path pinned above.
    assert!(matches!(
        frozen.store(&mrb, 0, mrb.str_new(b"x").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn entries_visits_nothing_for_an_empty_array() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();

    // A length-0 walk yields no elements at all.
    assert_eq!(ary.entries().count(), 0);
}

#[test]
fn entries_walks_elements_first_to_last() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    for n in [1, 2, 3] {
        ary.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }

    // The count is exact up front (ExactSizeIterator), and the walk reads
    // the slots from the first to the last in order.
    assert_eq!(ary.entries().len(), 3);
    let rendered: Vec<String> = ary.entries().map(|v| v.to_string(&mrb)).collect();
    assert_eq!(rendered, ["1", "2", "3"]);
}

#[test]
fn entries_snapshots_the_length_so_a_shrink_reads_nil_past_the_new_end() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    for n in [1, 2, 3] {
        ary.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }

    // The walk fixes its length at 3 when it begins. Shrinking the array
    // to one element mid-walk does not shorten the walk: the first slot
    // reads its live value, and the two positions the array no longer
    // reaches read nil. Re-reading the length each step would instead have
    // stopped after the single live element.
    let mut walk = ary.entries();
    assert_eq!(
        walk.next()
            .expect("the first slot is visited")
            .to_string(&mrb),
        "1"
    );

    ary.pop(&mrb).expect("pop succeeds");
    ary.pop(&mrb).expect("pop succeeds");
    assert_eq!(ary.len(), 1);

    assert!(walk
        .next()
        .expect("the second slot is still visited")
        .is_nil());
    assert!(walk
        .next()
        .expect("the third slot is still visited")
        .is_nil());
    assert!(walk.next().is_none());
}

#[test]
fn entries_does_not_visit_elements_appended_after_the_walk_begins() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    ary.push(&mrb, beni::Value::from_int(&mrb, 1))
        .expect("push succeeds");

    // The walk fixes its length at 1 when it begins. Growing the array
    // mid-walk does not lengthen the walk: it ends after the one element
    // present at the start, never reaching the appended tail.
    let mut walk = ary.entries();
    assert_eq!(
        walk.next()
            .expect("the first slot is visited")
            .to_string(&mrb),
        "1"
    );

    for n in [2, 3] {
        ary.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }
    assert_eq!(ary.len(), 3);

    assert!(walk.next().is_none());
}

#[test]
fn entries_reads_a_slot_changed_mid_walk_as_its_current_value() {
    let mrb = open_mrb();
    let ary = mrb.ary_new();
    for n in [1, 2, 3] {
        ary.push(&mrb, beni::Value::from_int(&mrb, n))
            .expect("push succeeds");
    }

    // The walk reads each slot live, not from a snapshot taken at the
    // start. Overwriting a not-yet-visited slot mid-walk therefore
    // surfaces its current value when the walk reaches it — a content
    // snapshot would instead yield the value the slot held at the start.
    let mut walk = ary.entries();
    assert_eq!(
        walk.next()
            .expect("the first slot is visited")
            .to_string(&mrb),
        "1"
    );

    ary.store(&mrb, 2, mrb.str_new(b"changed").as_value())
        .expect("an in-range store succeeds");

    assert_eq!(
        walk.next()
            .expect("the second slot is visited")
            .to_string(&mrb),
        "2"
    );
    assert_eq!(
        walk.next()
            .expect("the third slot is visited")
            .to_string(&mrb),
        "changed"
    );
    assert!(walk.next().is_none());
}
