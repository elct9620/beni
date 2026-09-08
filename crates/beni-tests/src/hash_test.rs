use beni::Mrb;

#[test]
fn set_and_get_roundtrip_with_nil_for_an_absent_key() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let hash = mrb.hash_new();

    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("assigning into a fresh hash succeeds");
    let keys = hash.keys(&mrb);

    assert_eq!(keys.entry(0).to_string(&mrb), "k");
    assert!(keys.entry(1).is_nil());
}

#[test]
fn set_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, Hash};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_hash.rb").expect("allocating the context must succeed");

    // A frozen Hash still carries the Hash tag, so the downcast holds,
    // but assigning into it raises FrozenError — which protect catches
    // into Err rather than long-jumping.
    let frozen = Hash::from_value(cxt.load_nstring(b"{}.freeze"))
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
    use beni::{Ccontext, Error, Value};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"raising_key.rb").expect("allocating the context must succeed");

    // mruby locates a key by dispatching its `hash` and `eql?`; a key
    // that raises in both drives every keyed operation through the
    // dispatch protect must catch rather than long-jump. A seeded
    // entry forces the comparison to run even on a small hash.
    let key = cxt.load_nstring(
        b"class BeniBoomKey; def hash; raise 'no'; end; def eql?(o); raise 'no'; end; end; BeniBoomKey.new",
    );
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the key class must not raise"
    );

    let hash = mrb.hash_new();
    hash.set(&mrb, mrb.str_new(b"seed").as_value(), Value::nil())
        .expect("seeding a plain key does not raise");

    let v = mrb.str_new(b"v").as_value();
    assert!(matches!(hash.set(&mrb, key, v), Err(Error::Exception(_))));
    assert!(matches!(hash.get(&mrb, key), Err(Error::Exception(_))));
    assert!(matches!(
        hash.contains_key(&mrb, key),
        Err(Error::Exception(_))
    ));
    assert!(matches!(
        hash.fetch(&mrb, key, Value::nil()),
        Err(Error::Exception(_))
    ));
    assert!(matches!(hash.delete(&mrb, key), Err(Error::Exception(_))));
}

#[test]
fn read_surfaces_a_raising_default_as_err() {
    use beni::{Ccontext, Error, FromValue, Hash};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt =
        Ccontext::new(&mrb, c"raising_default.rb").expect("allocating the context must succeed");

    // A hash whose default block raises turns an absent-key read into
    // a raise protect must catch — the default path a read takes and
    // fetch does not.
    let hash = Hash::from_value(cxt.load_nstring(b"Hash.new { raise 'no' }"))
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    assert_eq!(hash.values(&mrb).entry(0).to_string(&mrb), "v");
}

#[test]
fn contains_key_and_fetch_read_by_key() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    use beni::{Ccontext, Error, FromValue, Hash};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_clear.rb").expect("allocating the context must succeed");

    // clear checks frozen state before touching entries, so even a
    // populated frozen hash surfaces FrozenError as Err.
    let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
        .expect("a frozen Hash literal is Hash-tagged");
    assert!(matches!(frozen.clear(&mrb), Err(Error::Exception(_))));
}

#[test]
fn dup_copies_independently() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    use beni::{Ccontext, Error, FromValue, Hash};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_del.rb").expect("allocating the context must succeed");

    // delete checks frozen state before touching entries, so even a
    // populated frozen hash surfaces FrozenError as Err.
    let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
        .expect("a frozen Hash literal is Hash-tagged");
    assert!(matches!(
        frozen.delete(&mrb, mrb.str_new(b"a").as_value()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn update_surfaces_frozen_receiver_as_err() {
    use beni::{Ccontext, Error, FromValue, Hash};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt =
        Ccontext::new(&mrb, c"frozen_update.rb").expect("allocating the context must succeed");

    // merge checks frozen state before folding entries, so merging
    // into a frozen hash surfaces FrozenError as Err.
    let frozen = Hash::from_value(cxt.load_nstring(b"{a: 1}.freeze"))
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

#[test]
fn each_visits_every_pair_in_insertion_order() {
    use beni::{ForEach, Value};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let hash = mrb.hash_new();
    hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"c").as_value(), Value::from_int(&mrb, 3))
        .expect("set succeeds");

    let mut seen = Vec::new();
    hash.each(&mrb, |key, val| {
        seen.push((key.to_string(&mrb), val.to_string(&mrb)));
        ForEach::Continue
    })
    .expect("a read-only walk does not raise");

    assert_eq!(
        seen,
        vec![
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
            ("c".to_owned(), "3".to_owned()),
        ]
    );
}

#[test]
fn each_stops_early_on_stop() {
    use beni::{ForEach, Value};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let hash = mrb.hash_new();
    hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"c").as_value(), Value::from_int(&mrb, 3))
        .expect("set succeeds");

    // Stopping at the first pair leaves the rest unvisited.
    let mut count = 0;
    hash.each(&mrb, |_, _| {
        count += 1;
        ForEach::Stop
    })
    .expect("an early-stopping walk does not raise");

    assert_eq!(count, 1);
}

#[test]
fn each_surfaces_an_in_walk_modification_as_err() {
    use beni::{Error, ForEach, Value};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let hash = mrb.hash_new();
    hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
        .expect("set succeeds");

    // A closure that re-enters the VM to clear the hash it is walking
    // resets the entry table, so the guard mruby runs before the next
    // callback raises RuntimeError. protect catches that into Err
    // rather than letting it long-jump across mrb_hash_foreach's FFI
    // frame.
    let result = hash.each(&mrb, |_, _| {
        hash.clear(&mrb)
            .expect("the in-walk clear itself does not raise");
        ForEach::Continue
    });
    assert!(matches!(result, Err(Error::Exception(_))));

    // The VM survives the caught raise and stays usable: a fresh
    // operation runs without crashing.
    let other = mrb.hash_new();
    other
        .set(&mrb, mrb.str_new(b"x").as_value(), Value::from_int(&mrb, 9))
        .expect("the VM is usable after the protected raise");
    assert_eq!(other.len(&mrb), 1);
}

#[test]
fn each_resurfaces_a_closure_panic_on_the_rust_side() {
    use beni::Value;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let hash = mrb.hash_new();
    hash.set(&mrb, mrb.str_new(b"a").as_value(), Value::from_int(&mrb, 1))
        .expect("set succeeds");
    hash.set(&mrb, mrb.str_new(b"b").as_value(), Value::from_int(&mrb, 2))
        .expect("set succeeds");

    // A panic in the closure is caught at the FFI boundary, stops the
    // walk, and resumes here once mrb_hash_foreach returns — never
    // unwinding through mruby's C frames. catch_unwind sees the
    // resumed panic, proving it crossed back to the Rust side intact.
    let visited = std::cell::Cell::new(0u32);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // The closure panic resurfaces before `each` returns a Result,
        // so the value is never produced — bind it to silence must_use.
        let _ = hash.each(&mrb, |_, _| {
            visited.set(visited.get() + 1);
            panic!("boom in each closure");
        });
    }));

    let payload = result.expect_err("the closure panic must resurface Rust-side");
    let msg = payload
        .downcast_ref::<&str>()
        .copied()
        .expect("the original panic payload survives the round-trip");
    assert_eq!(msg, "boom in each closure");
    // The walk stopped at the first pair rather than running on.
    assert_eq!(visited.get(), 1);

    // The VM survives the caught panic.
    assert_eq!(hash.len(&mrb), 2);
}
