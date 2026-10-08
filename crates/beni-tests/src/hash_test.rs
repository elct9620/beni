use crate::support::open_mrb;
use beni::prelude::*;
use beni::{IntoValue, Value};

#[test]
fn set_and_get_roundtrip_with_nil_for_an_absent_key() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();

    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("assigning into a fresh hash succeeds");

    assert_eq!(
        hash.get(&mrb, mrb.str_new(b"k").as_value())
            .expect("a present string key reads without raising")
            .to_string(&mrb),
        "v"
    );
    assert!(hash
        .get(&mrb, mrb.str_new(b"absent").as_value())
        .expect("an absent string key reads without raising")
        .is_nil());
}

#[test]
fn keys_returns_the_typed_key_array() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();

    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("assigning into a fresh hash succeeds");
    let keys = hash.keys(&mrb);

    assert_eq!(keys.entry(&mrb, 0).to_string(&mrb), "k");
    assert!(keys.entry(&mrb, 1).is_nil());
}

#[test]
fn set_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, RHash};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"frozen_hash.rb").expect("allocating the context must succeed");

    // A frozen Hash still carries the Hash tag, so the downcast holds,
    // but assigning into it raises FrozenError — which protect catches
    // into Err rather than long-jumping.
    let frozen = RHash::from_value(
        cxt.load_nstring(b"{}.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Hash literal is Hash-tagged");
    assert!(matches!(
        frozen.set(
            &mrb,
            mrb.str_new(b"k").as_value(),
            mrb.str_new(b"v").as_value()
        ),
        Err(Error::Exception(_))
    ));
}

#[test]
fn keyed_operations_surface_a_raising_key_as_err() {
    use beni::{Ccontext, Error};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"raising_key.rb").expect("allocating the context must succeed");

    // mruby locates a key by dispatching its `hash` and `eql?`; a key
    // that raises in both drives every keyed operation through the
    // dispatch protect must catch rather than long-jump. A seeded
    // entry forces the comparison to run even on a small hash.
    let key = cxt.load_nstring(
        b"class BeniBoomKey; def hash; raise 'no'; end; def eql?(o); raise 'no'; end; end; BeniBoomKey.new",
    ).expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the key class must not raise"
    );

    let hash = mrb.hash_new();
    hash.set(
        &mrb,
        mrb.str_new(b"seed").as_value(),
        beni::value::qnil().as_value(),
    )
    .expect("seeding a plain key does not raise");

    let v = mrb.str_new(b"v").as_value();
    assert!(matches!(hash.set(&mrb, key, v), Err(Error::Exception(_))));
    assert!(matches!(hash.get(&mrb, key), Err(Error::Exception(_))));
    assert!(matches!(
        hash.contains_key(&mrb, key),
        Err(Error::Exception(_))
    ));
    assert!(matches!(
        hash.fetch(&mrb, key, beni::value::qnil().as_value()),
        Err(Error::Exception(_))
    ));
    assert!(matches!(hash.delete(&mrb, key), Err(Error::Exception(_))));
}

#[test]
fn read_surfaces_a_raising_default_as_err() {
    use beni::{Ccontext, Error, FromValue, RHash};

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"raising_default.rb").expect("allocating the context must succeed");

    // A hash whose default block raises turns an absent-key read into
    // a raise protect must catch — the default path a read takes and
    // fetch does not.
    let hash = RHash::from_value(
        cxt.load_nstring(b"Hash.new { raise 'no' }")
            .expect("the test source must compile and run"),
    )
    .expect("a Hash is Hash-tagged");
    assert!(
        mrb.pending_exc().is_nil(),
        "building the hash must not raise"
    );

    assert!(matches!(
        hash.get(&mrb, mrb.str_new(b"absent").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn values_size_and_emptiness_read_the_structure() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    assert!(hash.is_empty(&mrb));
    assert_eq!(hash.len(&mrb), 0);

    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("set succeeds");

    assert_eq!(hash.len(&mrb), 1);
    assert!(!hash.is_empty(&mrb));
    assert_eq!(hash.values(&mrb).entry(&mrb, 0).to_string(&mrb), "v");
}

#[test]
fn contains_key_and_fetch_read_by_key() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    let k = mrb.str_new(b"k").as_value();
    hash.set(&mrb, k, mrb.str_new(b"v").as_value())
        .expect("set succeeds");

    assert!(hash.contains_key(&mrb, k).expect("key test succeeds"));
    assert!(!hash
        .contains_key(&mrb, mrb.str_new(b"absent").as_value())
        .expect("key test succeeds"));

    assert_eq!(
        hash.fetch(&mrb, k, mrb.str_new(b"def").as_value())
            .expect("fetch succeeds")
            .to_string(&mrb),
        "v"
    );
    // An absent key returns the supplied default, not an error.
    assert_eq!(
        hash.fetch(
            &mrb,
            mrb.str_new(b"absent").as_value(),
            mrb.str_new(b"def").as_value(),
        )
        .expect("fetch succeeds")
        .to_string(&mrb),
        "def"
    );
}

#[test]
fn delete_removes_and_update_merges() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    let k = mrb.str_new(b"k").as_value();
    hash.set(&mrb, k, mrb.str_new(b"v").as_value())
        .expect("set succeeds");

    assert_eq!(
        hash.delete(&mrb, k)
            .expect("delete succeeds")
            .to_string(&mrb),
        "v"
    );
    assert!(hash.is_empty(&mrb));

    let other = mrb.hash_new();
    other
        .set(
            &mrb,
            mrb.str_new(b"a").as_value(),
            mrb.str_new(b"1").as_value(),
        )
        .expect("set succeeds");
    hash.update(&mrb, other).expect("update succeeds");
    assert_eq!(
        hash.get(&mrb, mrb.str_new(b"a").as_value())
            .expect("a present string key reads without raising")
            .to_string(&mrb),
        "1"
    );
}

#[test]
fn clear_empties_the_hash() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("set succeeds");
    assert!(!hash.is_empty(&mrb));

    hash.clear(&mrb).expect("clear succeeds");
    assert!(hash.is_empty(&mrb));
}

#[test]
fn clear_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, RHash};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"frozen_clear.rb").expect("allocating the context must succeed");

    // clear checks frozen state before touching entries, so even a
    // populated frozen hash surfaces FrozenError as Err.
    let frozen = RHash::from_value(
        cxt.load_nstring(b"{a: 1}.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Hash literal is Hash-tagged");
    assert!(matches!(frozen.clear(&mrb), Err(Error::Exception(_))));
}

#[test]
fn dup_copies_independently() {
    let mrb = open_mrb();
    let hash = mrb.hash_new();
    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("set succeeds");

    let copy = hash.dup(&mrb);
    hash.delete(&mrb, mrb.str_new(b"k").as_value())
        .expect("delete succeeds");

    // Mutating the original leaves the dup untouched.
    assert!(hash.is_empty(&mrb));
    assert_eq!(
        copy.get(&mrb, mrb.str_new(b"k").as_value())
            .expect("a present string key reads without raising")
            .to_string(&mrb),
        "v"
    );
}

#[test]
fn delete_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, RHash};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"frozen_del.rb").expect("allocating the context must succeed");

    // delete checks frozen state before touching entries, so even a
    // populated frozen hash surfaces FrozenError as Err.
    let frozen = RHash::from_value(
        cxt.load_nstring(b"{a: 1}.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Hash literal is Hash-tagged");
    assert!(matches!(
        frozen.delete(&mrb, mrb.str_new(b"a").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn update_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, RHash};

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"frozen_update.rb").expect("allocating the context must succeed");

    // merge checks frozen state before folding entries, so merging
    // into a frozen hash surfaces FrozenError as Err.
    let frozen = RHash::from_value(
        cxt.load_nstring(b"{a: 1}.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen Hash literal is Hash-tagged");
    let other = mrb.hash_new();
    other
        .set(
            &mrb,
            mrb.str_new(b"b").as_value(),
            mrb.str_new(b"2").as_value(),
        )
        .expect("set succeeds");
    assert!(matches!(
        frozen.update(&mrb, other),
        Err(Error::Exception(_))
    ));
}

fn abc_hash(mrb: &beni::Mrb) -> beni::RHash {
    let hash = mrb.hash_new();
    for (key, val) in [(b"a", 1i32), (b"b", 2), (b"c", 3)] {
        hash.set(mrb, mrb.str_new(key).as_value(), val.into_value(mrb))
            .expect("set succeeds");
    }
    hash
}

#[test]
fn foreach_hands_each_pair_converted_in_insertion_order() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let mut seen = Vec::new();
    hash.foreach(&mrb, |key: String, val: i32| {
        seen.push((key, val));
        Ok(ForEach::Continue)
    })
    .expect("a read-only walk does not raise");

    assert_eq!(
        seen,
        vec![
            ("a".to_owned(), 1),
            ("b".to_owned(), 2),
            ("c".to_owned(), 3)
        ]
    );
}

#[test]
fn foreach_stops_early_on_stop() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let mut count = 0;
    hash.foreach(&mrb, |_: Value, _: Value| {
        count += 1;
        Ok(ForEach::Stop)
    })
    .expect("an early-stopping walk does not raise");

    assert_eq!(count, 1);
}

#[test]
fn foreach_ends_with_a_conversion_or_closure_err() {
    use beni::{Error, ForEach};

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let mut visited = 0;
    let err = hash
        .foreach(&mrb, |_: i32, _: i32| {
            visited += 1;
            Ok(ForEach::Continue)
        })
        .expect_err("a String key does not convert to i32");
    assert_eq!(
        visited, 0,
        "the walk ends at the pair that fails to convert"
    );
    assert_eq!(err.message(&mrb), "String cannot be converted to Integer");

    let mut visited = 0;
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| -> Result<ForEach, Error> {
            visited += 1;
            Err(Error::Panic("stop here".to_owned()))
        })
        .expect_err("the closure's Err surfaces");
    assert_eq!(visited, 1);
    assert!(matches!(err, Error::Panic(msg) if msg == "stop here"));
}

fn assert_hash_modified(mrb: &beni::Mrb, err: &beni::Error) {
    let beni::Error::Exception(exc) = err else {
        panic!("a pair-count change surfaces mruby's exception, got {err:?}");
    };
    assert_eq!(exc.classname(mrb), "RuntimeError");
    assert_eq!(err.message(mrb), "hash modified");
}

#[test]
fn foreach_ends_when_a_visit_deletes_a_pair_ahead_of_it() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let mut visited = 0;
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| {
            visited += 1;
            hash.delete(&mrb, mrb.str_new(b"c").as_value())
                .expect("the in-walk delete itself does not raise");
            Ok(ForEach::Continue)
        })
        .expect_err("a visit that changes the pair count ends the walk");

    assert_hash_modified(&mrb, &err);
    assert_eq!(visited, 1, "the walk ends at the visit that deleted");
}

#[test]
fn foreach_ends_when_a_visit_adds_a_pair() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let mut visited = 0;
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| {
            visited += 1;
            hash.set(&mrb, mrb.str_new(b"d").as_value(), 4i32.into_value(&mrb))
                .expect("the in-walk set itself does not raise");
            Ok(ForEach::Continue)
        })
        .expect_err("a visit that changes the pair count ends the walk");

    assert_hash_modified(&mrb, &err);
    assert_eq!(visited, 1);
}

/// An owned target whose conversion re-enters the VM to delete the pair
/// keyed `"c"` from the hash `$beni_walked` names.
struct DeletingOnConvert;

impl beni::TryConvert for DeletingOnConvert {
    fn try_convert(val: Value, mrb: &beni::Mrb) -> Result<Self, beni::Error> {
        let cxt = beni::Ccontext::new(mrb, c"deleting_on_convert.rb")
            .expect("allocating the compile context must succeed");
        cxt.load_nstring(b"$beni_walked.delete('c')")
            .expect("the delete runs");
        i32::try_convert(val, mrb)?;
        Ok(Self)
    }
}

// SAFETY: a `DeletingOnConvert` holds an integer and no `Value`.
unsafe impl beni::TryConvertOwned for DeletingOnConvert {}

#[test]
fn a_map_conversion_ends_when_a_conversion_changes_the_pair_count() {
    let mrb = open_mrb();
    let hash = abc_hash(&mrb);
    mrb.gv_set(c"$beni_walked", hash.as_value())
        .expect("binding the global succeeds");

    let Err(err) = hash.to_hash_map::<String, DeletingOnConvert>(&mrb) else {
        panic!("a conversion that changes the pair count ends the walk");
    };

    assert_hash_modified(&mrb, &err);
}

#[test]
fn foreach_ends_when_a_visit_clears_the_hash() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let err = hash
        .foreach(&mrb, |_: Value, _: Value| {
            hash.clear(&mrb)
                .expect("the in-walk clear itself does not raise");
            Ok(ForEach::Continue)
        })
        .expect_err("a visit that changes the pair count ends the walk");

    assert_hash_modified(&mrb, &err);
    let other = mrb.hash_new();
    other
        .set(&mrb, mrb.str_new(b"x").as_value(), 9i32.into_value(&mrb))
        .expect("the VM is usable after the walk ends");
    assert_eq!(other.len(&mrb), 1);
}

#[test]
fn a_pair_count_change_ends_the_walk_even_when_the_visit_stops_it() {
    use beni::ForEach;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let err = hash
        .foreach(&mrb, |_: Value, _: Value| {
            hash.delete(&mrb, mrb.str_new(b"c").as_value())
                .expect("the in-walk delete itself does not raise");
            Ok(ForEach::Stop)
        })
        .expect_err("the count change outranks the stop");

    assert_hash_modified(&mrb, &err);
}

#[test]
fn the_closures_own_err_or_panic_outranks_a_pair_count_change() {
    use beni::{Error, ForEach};

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| -> Result<ForEach, Error> {
            hash.delete(&mrb, mrb.str_new(b"c").as_value())
                .expect("the in-walk delete itself does not raise");
            Err(Error::Panic("the closure's own".to_owned()))
        })
        .expect_err("the closure's Err surfaces");
    assert!(matches!(err, Error::Panic(msg) if msg == "the closure's own"));

    let hash = abc_hash(&mrb);
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| -> Result<ForEach, Error> {
            hash.delete(&mrb, mrb.str_new(b"c").as_value())
                .expect("the in-walk delete itself does not raise");
            panic!("boom after deleting");
        })
        .expect_err("the closure's panic surfaces");
    assert!(matches!(err, Error::Panic(msg) if msg == "boom after deleting"));
}

#[test]
fn a_visit_swapping_pairs_hands_over_only_pairs_the_hash_held() {
    use beni::ForEach;

    let mrb = open_mrb();
    // Keys 0, 2, 3 behind the slot key 1 vacated: a visit that deletes
    // one pair and adds another keeps the count while the entries move
    // under the walk.
    let hash = mrb.hash_new();
    for key in 0..4i32 {
        hash.set(&mrb, key.into_value(&mrb), key.into_value(&mrb))
            .expect("set succeeds");
    }
    hash.delete(&mrb, 1i32.into_value(&mrb))
        .expect("delete succeeds");

    let held = [0, 2, 3, 100];
    let mut seen = Vec::new();
    let walked = hash.foreach(&mrb, |key: i32, _: Value| {
        seen.push(key);
        if seen.len() == 2 {
            hash.delete(&mrb, 3i32.into_value(&mrb))
                .expect("the in-walk delete itself does not raise");
            hash.set(&mrb, 100i32.into_value(&mrb), 0i32.into_value(&mrb))
                .expect("the in-walk set itself does not raise");
        }
        Ok(ForEach::Continue)
    });

    if let Err(err) = &walked {
        assert_hash_modified(&mrb, err);
    }
    assert!(
        seen.iter().all(|key| held.contains(key)),
        "every pair handed over was held during the walk: {seen:?}"
    );
    let mut unique = seen.clone();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "no pair is handed over twice: {seen:?}"
    );
}

#[test]
fn foreach_answers_a_closure_panic_as_err() {
    use beni::Error;

    let mrb = open_mrb();
    let hash = abc_hash(&mrb);

    let visited = std::cell::Cell::new(0u32);
    let err = hash
        .foreach(&mrb, |_: Value, _: Value| -> Result<beni::ForEach, Error> {
            visited.set(visited.get() + 1);
            panic!("boom in foreach closure");
        })
        .expect_err("the closure panic surfaces as an Err");

    assert!(matches!(err, Error::Panic(msg) if msg == "boom in foreach closure"));
    assert_eq!(visited.get(), 1, "the walk stopped at the first pair");
    assert_eq!(hash.len(&mrb), 3, "the VM survives the caught panic");
}
