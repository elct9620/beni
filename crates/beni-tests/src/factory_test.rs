use crate::support::open_mrb;

#[test]
fn str_factories_roundtrip_their_bytes() {
    let mrb = open_mrb();

    assert_eq!(
        mrb.str_new(b"from bytes").as_value().to_string(&mrb),
        "from bytes"
    );
    assert_eq!(
        mrb.str_new_cstr(c"from cstr").as_value().to_string(&mrb),
        "from cstr"
    );
}

#[test]
fn str_new_capa_preallocates_an_empty_string() {
    let mrb = open_mrb();

    // Capacity is a hint, not content — the string starts empty and
    // fills as usual through cat.
    let s = mrb.str_new_capa(16);
    assert!(s.is_empty());
    s.cat(&mrb, b"reserved")
        .expect("append to a fresh string succeeds");
    assert_eq!(s.to_bytes(), b"reserved".to_vec());
}

#[test]
fn str_new_static_aliases_a_static_buffer_without_copying() {
    let mrb = open_mrb();

    // A byte-string literal is a `&'static [u8]`, the same path mruby's
    // `mrb_str_new_lit` macro takes.
    let s = mrb.str_new_static(b"borrowed");
    assert_eq!(s.len(), 8);
    assert_eq!(s.to_bytes(), b"borrowed".to_vec());
}

#[test]
fn str_new_static_copies_on_in_place_write() {
    let mrb = open_mrb();

    // Appending reallocates the copy-on-write string before mutating,
    // so the in-place op yields the grown result without touching the
    // borrowed buffer.
    let s = mrb.str_new_static(b"static");
    s.cat(&mrb, b"+more")
        .expect("appending to a static-backed string succeeds after copy");
    assert_eq!(s.to_bytes(), b"static+more".to_vec());

    // Resize likewise reallocates first; shrinking drops the tail.
    let r = mrb.str_new_static(b"Hello, world!");
    r.resize(&mrb, 5)
        .expect("resizing a static-backed string succeeds");
    assert_eq!(r.to_bytes(), b"Hello".to_vec());
}

#[test]
fn ary_new_capa_preallocates_an_empty_array() {
    let mrb = open_mrb();

    // Capacity is a hint, not content — the array starts empty and
    // fills as usual.
    let ary = mrb.ary_new_capa(8);
    assert!(ary.is_empty());
    ary.push(&mrb, mrb.str_new(b"x").as_value())
        .expect("push to a fresh array succeeds");
    assert_eq!(ary.len(), 1);
}

#[test]
fn hash_new_capa_preallocates_an_empty_hash() {
    let mrb = open_mrb();

    // Capacity is a hint, not content — the hash starts empty and
    // fills as usual.
    let hash = mrb.hash_new_capa(8);
    assert!(hash.is_empty(&mrb));
    hash.set(
        &mrb,
        mrb.str_new(b"k").as_value(),
        mrb.str_new(b"v").as_value(),
    )
    .expect("set to a fresh hash succeeds");
    assert_eq!(hash.len(&mrb), 1);
}

#[test]
fn ary_new_from_values_copies_the_slice_in_order() {
    let mrb = open_mrb();

    let values = [
        mrb.str_new(b"a").as_value(),
        mrb.str_new(b"b").as_value(),
        mrb.str_new(b"c").as_value(),
    ];
    let ary = mrb.ary_new_from_values(&values);

    assert_eq!(ary.len(), 3);
    assert_eq!(ary.entry(0).to_string(&mrb), "a");
    assert_eq!(ary.entry(2).to_string(&mrb), "c");
}

#[test]
fn assoc_new_pairs_the_two_values_in_order() {
    let mrb = open_mrb();

    let pair = mrb.assoc_new(
        mrb.str_new(b"car").as_value(),
        mrb.str_new(b"cdr").as_value(),
    );

    assert_eq!(pair.len(), 2);
    assert_eq!(pair.entry(0).to_string(&mrb), "car");
    assert_eq!(pair.entry(1).to_string(&mrb), "cdr");
}
