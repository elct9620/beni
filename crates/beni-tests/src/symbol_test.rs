use crate::support::open_mrb;
use beni::{FromValue, IntoValue, Symbol};

#[test]
fn name_sym_and_rebuild_roundtrip() {
    let mrb = open_mrb();
    let sym = Symbol::new(&mrb, c"flags");

    assert_eq!(sym.name(&mrb).as_deref(), Some("flags"));
    // The unboxed id must equal interning the same name — a wrong
    // boxing shift in the unbox shim would diverge here.
    assert_eq!(sym.to_sym(), mrb.intern_cstr(c"flags"));
    // Re-boxing the id yields an equal symbol.
    assert_eq!(
        Symbol::from_sym(sym.to_sym()).name(&mrb).as_deref(),
        Some("flags")
    );
}

#[test]
fn name_bytes_and_dump_read_the_symbol_name() {
    let mrb = open_mrb();

    // A plain identifier: bytes equal the name, dump is bare.
    let plain = Symbol::new(&mrb, c"fred");
    assert_eq!(plain.name_bytes(&mrb).as_deref(), Some(&b"fred"[..]));
    assert_eq!(plain.dump(&mrb).as_deref(), Some("fred"));

    // An embedded NUL: `name` escapes it to the dump form, only
    // `name_bytes` returns the raw bytes.
    let nul = Symbol::from_sym(mrb.intern_str(mrb.str_new(b"a\0b").as_value()));
    assert_eq!(nul.name(&mrb).as_deref(), Some("\"a\\x00b\""));
    assert_eq!(nul.name_bytes(&mrb).as_deref(), Some(&b"a\0b"[..]));

    // A name needing escaping dumps quoted.
    let spaced = Symbol::from_sym(mrb.intern_str(mrb.str_new(b"a b").as_value()));
    assert_eq!(spaced.dump(&mrb).as_deref(), Some("\"a b\""));
}

#[test]
fn name_copies_short_inline_names_out_of_the_shared_scratch_buffer() {
    let mrb = open_mrb();

    // Two short names pack inline (<=4 packable chars), so mruby unpacks
    // each into one shared per-VM scratch buffer that the next name read
    // overwrites. Read both, holding the first across the second read.
    // A borrowed return would alias the buffer and show the first name
    // mutated to the second; the owned copy must stay intact.
    let first = Symbol::new(&mrb, c"aa").name(&mrb).expect("aa has a name");
    let second = Symbol::new(&mrb, c"bb").name(&mrb).expect("bb has a name");

    assert_eq!(first, "aa");
    assert_eq!(second, "bb");
}

#[test]
fn to_str_reifies_the_name_as_a_mutable_string() {
    let mrb = open_mrb();
    let sym = Symbol::new(&mrb, c"flags");

    // The reified String carries the symbol's name bytes verbatim.
    let str = sym.to_str(&mrb);
    assert_eq!(str.to_bytes(), b"flags");

    // It is `Symbol#to_s`, not `#name`: the value is unfrozen, so a
    // consumer may mutate it — `Symbol#name` would come back frozen.
    assert!(!str
        .as_value()
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());
}

#[test]
fn to_sym_coerces_symbol_string_and_rejects_others() {
    let mrb = open_mrb();

    // A symbol value coerces to the same symbol.
    let sym = Symbol::new(&mrb, c"key");
    let from_sym = sym.as_value().to_sym(&mrb).expect("a symbol value coerces");
    assert_eq!(from_sym.to_sym(), sym.to_sym());

    // A string value interns to the symbol of its contents — the id
    // matches interning the same name directly.
    let from_str = mrb
        .str_new(b"key")
        .as_value()
        .to_sym(&mrb)
        .expect("a string value coerces");
    assert_eq!(from_str.to_sym(), mrb.intern_cstr(c"key"));

    // A value that is neither a symbol nor a string rejects.
    assert!(42i32.into_value(&mrb).to_sym(&mrb).is_err());
}

#[test]
fn from_value_discriminates_the_symbol_tag() {
    let mrb = open_mrb();
    let sym_val = Symbol::new(&mrb, c"k").into_value(&mrb);

    assert!(Symbol::from_value(sym_val).is_some());
    // A non-symbol value — and an immediate — both reject.
    assert!(Symbol::from_value(mrb.str_new(b"k").as_value()).is_none());
    assert!(Symbol::from_value(42i32.into_value(&mrb)).is_none());
}
