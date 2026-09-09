use crate::support::open_mrb;
use beni::Symbol;

#[test]
fn intern_cstr_roundtrips_through_sym_name() {
    let mrb = open_mrb();

    let sym = mrb.intern_cstr(c"beni_sym");

    assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
}

#[test]
fn intern_str_yields_the_same_id_as_intern_cstr() {
    let mrb = open_mrb();

    let via_str = mrb.intern_str(mrb.str_new(b"beni_sym").as_value());

    assert_eq!(via_str, mrb.intern_cstr(c"beni_sym"));
}

#[test]
fn intern_interns_a_byte_slice_by_length_creating_the_symbol() {
    let mrb = open_mrb();

    // A runtime byte slice interns to a Symbol whose name round-trips,
    // and whose id equals interning the same name through the C-string
    // path — proving it's the same interned symbol.
    let sym = mrb.intern(b"beni_sym");
    assert_eq!(sym.name(&mrb).as_deref(), Some("beni_sym"));
    assert_eq!(sym.to_sym(), mrb.intern_cstr(c"beni_sym"));

    // It's length-based, not NUL-terminated: a slice carrying trailing
    // bytes past where a C string would stop interns those bytes too,
    // yielding a distinct symbol from the truncated name.
    let exact = b"abc";
    let padded = b"abc\0xyz";
    assert_eq!(
        mrb.intern(&exact[..]).name_bytes(&mrb).as_deref(),
        Some(&b"abc"[..])
    );
    assert_eq!(
        mrb.intern(&padded[..]).name_bytes(&mrb).as_deref(),
        Some(&b"abc\0xyz"[..])
    );
    assert_ne!(
        mrb.intern(&exact[..]).to_sym(),
        mrb.intern(&padded[..]).to_sym()
    );
}

#[test]
fn intern_static_yields_the_same_id_as_the_copying_intern() {
    let mrb = open_mrb();

    // A `b"..."` literal is the `&'static [u8]` the no-copy intern
    // borrows; the name must read back and resolve to the same id the
    // copying intern produces, proving it's the same interned symbol.
    let sym = mrb.intern_static(b"beni_sym");

    assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
    assert_eq!(sym, mrb.intern_cstr(c"beni_sym"));
}

#[test]
fn sym_name_len_carries_the_full_bytes_past_an_embedded_nul() {
    let mrb = open_mrb();

    // Intern a name with an embedded NUL through the bytes path; the
    // C-string `sym_name` would stop at the NUL, the length-carrying
    // read keeps the full bytes.
    let sym = mrb.intern_str(mrb.str_new(b"a\0b").as_value());

    // `sym_name` escapes a NUL-containing name to its dump form; only
    // the length-carrying read returns the raw bytes.
    assert_eq!(mrb.sym_name(sym).as_deref(), Some("\"a\\x00b\""));
    assert_eq!(mrb.sym_name_len(sym).as_deref(), Some(&b"a\0b"[..]));
}

#[test]
fn intern_check_finds_an_interned_name_and_misses_an_uninterned_one() {
    let mrb = open_mrb();

    // A name no one has interned yet has no symbol, so the check misses.
    assert!(mrb.intern_check(b"beni_unseen").is_none());

    // Once the name is interned, the check finds it and reports the
    // same id the creating intern produced.
    let id = mrb.intern_cstr(c"beni_seen");
    assert_eq!(mrb.intern_check(b"beni_seen").map(Symbol::to_sym), Some(id));
}

#[test]
fn sym_dump_quotes_a_non_identifier_name() {
    let mrb = open_mrb();

    // A plain identifier dumps bare; a name needing escaping is quoted.
    assert_eq!(
        mrb.sym_dump(mrb.intern_cstr(c"fred")).as_deref(),
        Some("fred")
    );
    assert_eq!(
        mrb.sym_dump(mrb.intern_str(mrb.str_new(b"a b").as_value()))
            .as_deref(),
        Some("\"a b\"")
    );
}

#[test]
fn sym_name_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
    let mrb = open_mrb();

    // Two short names pack inline (<=4 packable chars), so mruby unpacks
    // each into one shared per-VM scratch buffer that the next name read
    // overwrites. Read both, holding the first across the second read.
    // A borrowed return would alias the buffer and show the first name
    // mutated to the second; the owned copy must stay intact.
    let first = mrb.sym_name(mrb.intern_cstr(c"aa")).expect("aa has a name");
    let second = mrb.sym_name(mrb.intern_cstr(c"bb")).expect("bb has a name");

    assert_eq!(first, "aa");
    assert_eq!(second, "bb");
}

#[test]
fn sym_name_len_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
    let mrb = open_mrb();

    // Same aliasing net as `sym_name`: hold the first read's bytes
    // across a second inline-name read that overwrites the shared
    // scratch buffer; the owned copy must stay intact.
    let first = mrb
        .sym_name_len(mrb.intern_cstr(c"aa"))
        .expect("aa has a name");
    let second = mrb
        .sym_name_len(mrb.intern_cstr(c"bb"))
        .expect("bb has a name");

    assert_eq!(first, b"aa");
    assert_eq!(second, b"bb");
}

#[test]
fn sym_dump_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
    let mrb = open_mrb();

    // Same aliasing net as `sym_name`: hold the first dump across a
    // second inline-name dump that overwrites the shared scratch
    // buffer; the owned copy must stay intact.
    let first = mrb.sym_dump(mrb.intern_cstr(c"aa")).expect("aa dumps");
    let second = mrb.sym_dump(mrb.intern_cstr(c"bb")).expect("bb dumps");

    assert_eq!(first, "aa");
    assert_eq!(second, "bb");
}
