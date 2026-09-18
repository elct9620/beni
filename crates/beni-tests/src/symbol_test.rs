use crate::support::open_mrb;
use beni::prelude::*;
use beni::{FromValue, Id, IntoValue, Symbol};

#[test]
fn name_sym_and_rebuild_roundtrip() {
    let mrb = open_mrb();
    let sym = Symbol::new(&mrb, c"flags").expect("the name interns");

    assert_eq!(sym.name(&mrb).as_deref(), Some("flags"));
    // It must equal interning the same name — a wrong boxing shift in
    // the unbox shim would diverge here.
    assert_eq!(sym, mrb.intern_cstr(c"flags").expect("the name interns"));
    // Unboxing to the id and boxing it again yields an equal symbol.
    let reboxed = Symbol::from(Id::from(sym));
    assert_eq!(reboxed, sym);
    assert_eq!(reboxed.name(&mrb).as_deref(), Some("flags"));
}

#[test]
fn name_bytes_and_dump_read_the_symbol_name() {
    let mrb = open_mrb();

    // A plain identifier: bytes equal the name, dump is bare.
    let plain = Symbol::new(&mrb, c"fred").expect("the name interns");
    assert_eq!(plain.name_bytes(&mrb).as_deref(), Some(&b"fred"[..]));
    assert_eq!(plain.dump(&mrb).as_deref(), Some("fred"));

    // An embedded NUL: `name` escapes it to the dump form, only
    // `name_bytes` returns the raw bytes.
    let nul = Symbol::from(
        mrb.intern_str(mrb.str_new(b"a\0b").as_value())
            .expect("the name interns"),
    );
    assert_eq!(nul.name(&mrb).as_deref(), Some("\"a\\x00b\""));
    assert_eq!(nul.name_bytes(&mrb).as_deref(), Some(&b"a\0b"[..]));

    // A name needing escaping dumps quoted.
    let spaced = Symbol::from(
        mrb.intern_str(mrb.str_new(b"a b").as_value())
            .expect("the name interns"),
    );
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
    let first = Symbol::new(&mrb, c"aa")
        .expect("the name interns")
        .name(&mrb)
        .expect("aa has a name");
    let second = Symbol::new(&mrb, c"bb")
        .expect("the name interns")
        .name(&mrb)
        .expect("bb has a name");

    assert_eq!(first, "aa");
    assert_eq!(second, "bb");
}

#[test]
fn to_str_reifies_the_name_as_a_mutable_string() {
    let mrb = open_mrb();
    let sym = Symbol::new(&mrb, c"flags").expect("the name interns");

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
    let sym = Symbol::new(&mrb, c"key").expect("the name interns");
    let from_sym = sym.as_value().to_sym(&mrb).expect("a symbol value coerces");
    assert_eq!(from_sym, sym);

    // A string value interns to the symbol of its contents — the id
    // matches interning the same name directly.
    let from_str = mrb
        .str_new(b"key")
        .as_value()
        .to_sym(&mrb)
        .expect("a string value coerces");
    assert_eq!(from_str, mrb.intern_cstr(c"key").expect("the name interns"));

    // A value that is neither a symbol nor a string rejects.
    assert!(42i32.into_value(&mrb).to_sym(&mrb).is_err());
}

#[test]
fn from_value_discriminates_the_symbol_tag() {
    let mrb = open_mrb();
    let sym_val = Symbol::new(&mrb, c"k")
        .expect("the name interns")
        .into_value(&mrb);

    assert!(Symbol::from_value(sym_val).is_some());
    // A non-symbol value — and an immediate — both reject.
    assert!(Symbol::from_value(mrb.str_new(b"k").as_value()).is_none());
    assert!(Symbol::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn a_name_too_long_to_be_a_symbol_surfaces_as_argument_error() {
    let mrb = open_mrb();
    let argument_error = mrb
        .exc_get(c"ArgumentError")
        .expect("ArgumentError is built in");
    let bytes = vec![b'a'; u16::MAX as usize];
    let name = std::ffi::CString::new(bytes.clone()).expect("the name holds no NUL");

    let Err(via_new) = Symbol::new(&mrb, &name) else {
        panic!("interning the name must refuse it");
    };
    let Err(via_coercion) = mrb.str_new(&bytes).as_value().to_sym(&mrb) else {
        panic!("coercing the string must refuse it");
    };

    for err in [via_new, via_coercion] {
        assert!(err.is_kind_of(&mrb, argument_error));
    }
}

#[test]
fn a_symbol_and_its_id_convert_and_compare_across_the_split() {
    let mrb = open_mrb();
    let id = mrb.intern(b"beni_split").expect("the name interns");
    let other = mrb.intern(b"beni_split_other").expect("the name interns");

    // A symbol value reached from Ruby compares with the id interned
    // from Rust, in either direction, and against another symbol.
    let sym = Symbol::from_value(
        mrb.load_string(b":beni_split")
            .expect("the literal evaluates"),
    )
    .expect("a symbol literal is a Symbol");
    assert_eq!(sym, id);
    assert_eq!(id, sym);
    assert_ne!(sym, other);
    assert_eq!(sym, Symbol::from(id));

    // The id boxes into that same symbol value.
    let boxed = Symbol::from_value(id.into_value(&mrb)).expect("an id boxes into a Symbol");
    assert_eq!(boxed, sym);
}

#[test]
fn an_id_keys_a_named_operation_like_its_name() {
    let mrb = open_mrb();
    let receiver = mrb.str_new(b"beni").as_value();
    let id = mrb.intern(b"upcase").expect("the name interns");

    let by_id = receiver.funcall(&mrb, id, &[]).expect("the call runs");
    let by_name = receiver
        .funcall(&mrb, "upcase", &[])
        .expect("the call runs");

    assert_eq!(String::from_value(by_id), Some("BENI".to_owned()));
    assert_eq!(String::from_value(by_id), String::from_value(by_name));
}
