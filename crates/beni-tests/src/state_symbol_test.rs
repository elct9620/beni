use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Error, FromValue, Id, Module, Mrb, Symbol, Value};
use std::ffi::CString;

/// The shortest name mruby refuses to make a symbol: `UINT16_MAX` bytes.
const TOO_LONG: usize = u16::MAX as usize;

static TOO_LONG_STATIC: [u8; TOO_LONG] = [b'a'; TOO_LONG];

fn too_long_cstr() -> CString {
    CString::new(vec![b'a'; TOO_LONG]).expect("the name holds no NUL")
}

#[test]
fn intern_cstr_roundtrips_through_sym_name() {
    let mrb = open_mrb();

    let sym = mrb.intern_cstr(c"beni_sym").expect("the name interns");

    assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
}

#[test]
fn intern_str_yields_the_same_id_as_intern_cstr() {
    let mrb = open_mrb();

    let via_str = mrb
        .intern_str(mrb.str_new(b"beni_sym").as_value())
        .expect("the name interns");

    assert_eq!(
        via_str,
        mrb.intern_cstr(c"beni_sym").expect("the name interns")
    );
}

#[test]
fn intern_interns_a_byte_slice_by_length_creating_the_symbol() {
    let mrb = open_mrb();

    // A runtime byte slice interns to an id whose name round-trips,
    // and whose id equals interning the same name through the C-string
    // path — proving it's the same interned symbol.
    let sym = mrb.intern(b"beni_sym").expect("the name interns");
    assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
    assert_eq!(sym, mrb.intern_cstr(c"beni_sym").expect("the name interns"));

    // It's length-based, not NUL-terminated: a slice carrying trailing
    // bytes past where a C string would stop interns those bytes too,
    // yielding a distinct symbol from the truncated name.
    let exact = b"abc";
    let padded = b"abc\0xyz";
    assert_eq!(
        mrb.sym_name_len(mrb.intern(&exact[..]).expect("the name interns"))
            .as_deref(),
        Some(&b"abc"[..])
    );
    assert_eq!(
        mrb.sym_name_len(mrb.intern(&padded[..]).expect("the name interns"))
            .as_deref(),
        Some(&b"abc\0xyz"[..])
    );
    assert_ne!(
        mrb.intern(&exact[..]).expect("the name interns"),
        mrb.intern(&padded[..]).expect("the name interns")
    );
}

#[test]
fn intern_static_yields_the_same_id_as_the_copying_intern() {
    let mrb = open_mrb();

    // A `b"..."` literal is the `&'static [u8]` the no-copy intern
    // borrows; the name must read back and resolve to the same id the
    // copying intern produces, proving it's the same interned symbol.
    let sym = mrb.intern_static(b"beni_sym").expect("the name interns");

    assert_eq!(mrb.sym_name(sym).as_deref(), Some("beni_sym"));
    assert_eq!(sym, mrb.intern_cstr(c"beni_sym").expect("the name interns"));
}

#[test]
fn sym_name_len_carries_the_full_bytes_past_an_embedded_nul() {
    let mrb = open_mrb();

    // Intern a name with an embedded NUL through the bytes path; the
    // C-string `sym_name` would stop at the NUL, the length-carrying
    // read keeps the full bytes.
    let sym = mrb
        .intern_str(mrb.str_new(b"a\0b").as_value())
        .expect("the name interns");

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
    // same symbol the creating intern produced.
    let seen = mrb.intern_cstr(c"beni_seen").expect("the name interns");
    assert_eq!(mrb.intern_check(b"beni_seen"), Some(seen));
}

#[test]
fn sym_dump_quotes_a_non_identifier_name() {
    let mrb = open_mrb();

    // A plain identifier dumps bare; a name needing escaping is quoted.
    assert_eq!(
        mrb.sym_dump(mrb.intern_cstr(c"fred").expect("the name interns"))
            .as_deref(),
        Some("fred")
    );
    assert_eq!(
        mrb.sym_dump(
            mrb.intern_str(mrb.str_new(b"a b").as_value())
                .expect("the name interns")
        )
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
    let first = mrb
        .sym_name(mrb.intern_cstr(c"aa").expect("the name interns"))
        .expect("aa has a name");
    let second = mrb
        .sym_name(mrb.intern_cstr(c"bb").expect("the name interns"))
        .expect("bb has a name");

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
        .sym_name_len(mrb.intern_cstr(c"aa").expect("the name interns"))
        .expect("aa has a name");
    let second = mrb
        .sym_name_len(mrb.intern_cstr(c"bb").expect("the name interns"))
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
    let first = mrb
        .sym_dump(mrb.intern_cstr(c"aa").expect("the name interns"))
        .expect("aa dumps");
    let second = mrb
        .sym_dump(mrb.intern_cstr(c"bb").expect("the name interns"))
        .expect("bb dumps");

    assert_eq!(first, "aa");
    assert_eq!(second, "bb");
}

#[test]
fn intern_check_misses_a_name_too_long_to_intern_without_raising() {
    let mrb = open_mrb();
    let too_long = vec![b'a'; TOO_LONG];

    assert!(mrb.intern_check(&too_long).is_none());
}

#[test]
fn creating_interns_surface_a_name_too_long_to_be_a_symbol_as_argument_error() {
    let mrb = open_mrb();
    let argument_error = mrb
        .exc_get(c"ArgumentError")
        .expect("ArgumentError is built in");
    let bytes = vec![b'a'; TOO_LONG];

    let Err(via_cstr) = mrb.intern_cstr(&too_long_cstr()) else {
        panic!("the C-string intern must refuse the name");
    };
    let Err(via_slice) = mrb.intern(&bytes) else {
        panic!("the byte-slice intern must refuse the name");
    };
    let Err(via_str) = mrb.intern_str(mrb.str_new(&bytes).as_value()) else {
        panic!("the String-value intern must refuse the name");
    };
    let Err(via_static) = mrb.intern_static(&TOO_LONG_STATIC) else {
        panic!("the static-buffer intern must refuse the name");
    };

    for err in [via_cstr, via_slice, via_str, via_static] {
        assert!(err.is_kind_of(&mrb, argument_error));
    }
}

#[test]
fn creating_interns_intern_the_longest_name_a_symbol_holds() {
    let mrb = open_mrb();
    let longest = vec![b'a'; TOO_LONG - 1];

    let sym = mrb
        .intern(&longest)
        .expect("a name one byte short of the limit interns");

    assert_eq!(mrb.sym_name_len(sym).map(|b| b.len()), Some(TOO_LONG - 1));
}

/// A method body that interns a name too long to be a symbol and hands
/// the refusal back with `?`.
fn intern_a_name_too_long(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Ok(Symbol::new(mrb, &too_long_cstr())?.as_value())
}

#[test]
fn a_method_body_hands_an_over_long_intern_to_ruby_as_a_rescuable_argument_error() {
    let mrb = open_mrb();
    let class = mrb
        .define_class(c"BeniLongInternProbe", mrb.object_class())
        .expect("defining the probe class must succeed");
    class
        .define_method(&mrb, c"run", beni::method!(intern_a_name_too_long, 0))
        .expect("registering the probe method must succeed");

    let rescued = mrb
        .load_string(
            b"begin
                BeniLongInternProbe.new.run
                0
              rescue ArgumentError
                1
              rescue Exception
                2
              end",
        )
        .expect("the rescue answers without raising");

    assert_eq!(i32::from_value(rescued), Some(1));
}

#[test]
fn ids_compare_and_hash_by_the_name_they_carry() {
    let mrb = open_mrb();

    // Interning is canonical, so the route an id arrives by never shows
    // in the comparison: the same bytes are the same id whichever intern
    // produced them, and different bytes are not.
    let via_cstr = mrb.intern_cstr(c"beni_eq").expect("the name interns");
    let via_bytes = mrb.intern(b"beni_eq").expect("the name interns");
    let other = mrb.intern(b"beni_eq_other").expect("the name interns");
    assert_eq!(via_cstr, via_bytes);
    assert_ne!(via_cstr, other);

    // Equal ids hash alike, which is what lets one key a map.
    let mut seen = std::collections::HashMap::new();
    seen.insert(via_cstr, 1);
    assert_eq!(seen.get(&via_bytes), Some(&1));
    assert_eq!(seen.get(&other), None);
}

#[test]
fn a_raw_id_crosses_back_into_the_id_it_names() {
    let mrb = open_mrb();

    // The seam an id leaves the typed surface through and returns by:
    // the read is safe, the crossing back is the caller's to establish.
    let interned = mrb.intern(b"beni_seam").expect("the name interns");
    let raw = beni::sys::AsRawId::as_raw(interned);
    // SAFETY: `raw` came from an id `mrb` interned.
    let crossed = unsafe { <Id as beni::sys::FromRawId>::from_raw(raw) };

    assert_eq!(crossed, interned);
    assert_eq!(mrb.sym_name(crossed).as_deref(), Some("beni_seam"));
}
