use crate::support::open_mrb;
use beni::{Array, FromValue, Hash, IntoValue, RClass, RModule, RString, Value};

// Boxes through mruby's generic `mrb_int_value` / `mrb_float_value`
// constructors and unboxes through the macro-expanding C helpers —
// the full ABI-alignment path. A bindgen/archive layout mismatch
// (wrong defines fed to the trampoline compile) corrupts these
// roundtrips before anything else.
#[test]
fn scalars_roundtrip_through_a_live_vm() {
    let mrb = open_mrb();

    let int_val = 42i32.into_value(&mrb);
    assert_eq!(i32::from_value(int_val), Some(42));

    let float_val = 1.5f64.into_value(&mrb);
    assert_eq!(f64::from_value(float_val), Some(1.5));

    // Cross-type downcasts fail cleanly instead of misreading the
    // payload.
    assert_eq!(i32::from_value(float_val), None);
    assert_eq!(f64::from_value(int_val), None);
}

#[test]
fn bool_round_trips_and_converts_totally() {
    let mrb = open_mrb();

    // The two canonical booleans round-trip through IntoValue.
    assert_eq!(bool::from_value(true.into_value(&mrb)), Some(true));
    assert_eq!(bool::from_value(false.into_value(&mrb)), Some(false));
    // The conversion is total — it never returns None — and reads a
    // non-boolean through Ruby truthiness (`nil` is falsy). The full
    // truthiness boundary lives with `Value::to_bool` in value.rs.
    assert_eq!(bool::from_value(Value::nil()), Some(false));
}

#[test]
fn string_converts_utf8_and_rejects_otherwise() {
    let mrb = open_mrb();

    // A UTF-8 string value converts to an owned Rust String,
    // multi-byte characters included.
    let s = mrb.str_new("héllo".as_bytes()).as_value();
    assert_eq!(String::from_value(s), Some("héllo".to_string()));

    // A non-string tag rejects.
    assert_eq!(String::from_value(42i32.into_value(&mrb)), None);

    // A String-tagged value whose bytes are not valid UTF-8 rejects
    // — it cannot become a Rust String, whose invariant is UTF-8.
    let invalid = mrb.str_new(&[0xff, 0xfe]).as_value();
    assert_eq!(String::from_value(invalid), None);
}

#[test]
fn rstring_downcasts_by_tag() {
    let mrb = open_mrb();

    // A String-tagged value downcasts to the typed handle; a
    // non-string tag rejects instead of wrapping a value the
    // `mrb_str_*` calls would misread.
    let s = mrb.str_new(b"hi").as_value();
    assert_eq!(
        RString::from_value(s).map(|r| r.to_bytes()),
        Some(b"hi".to_vec())
    );
    assert!(RString::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn vec_u8_converts_arbitrary_bytes_and_rejects_non_string() {
    let mrb = open_mrb();

    // A String-tagged value yields its bytes verbatim — non-UTF-8
    // bytes that the owned `String` conversion rejects survive here.
    let binary = mrb.str_new(&[0xff, 0x00, 0xfe]).as_value();
    assert_eq!(Vec::<u8>::from_value(binary), Some(vec![0xff, 0x00, 0xfe]));
    // A non-string tag rejects, like every other downcast.
    assert_eq!(Vec::<u8>::from_value(42i32.into_value(&mrb)), None);
}

#[test]
fn container_downcasts_discriminate_by_tag() {
    let mrb = open_mrb();

    let ary = mrb.ary_new().as_value();
    let hash = mrb.hash_new().as_value();

    assert!(Array::from_value(ary).is_some());
    assert!(Hash::from_value(hash).is_some());

    // The wrong container tag — and a non-container tag — both
    // reject instead of wrapping a value the `mrb_ary_*` /
    // `mrb_hash_*` calls would misread.
    assert!(Array::from_value(hash).is_none());
    assert!(Hash::from_value(ary).is_none());
    assert!(Array::from_value(42i32.into_value(&mrb)).is_none());
    assert!(Hash::from_value(42i32.into_value(&mrb)).is_none());
}

#[test]
fn container_downcast_includes_subclass_instances() {
    let mrb = open_mrb();
    let cxt = beni::Ccontext::new(&mrb, c"convert_test.rb")
        .expect("allocating the compile context must succeed");

    let sub = cxt
        .load_nstring(b"class MyAry < Array; end; MyAry.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the Array subclass must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // The tag, not the classname, decides: the instance reports
    // its subclass name yet converts and operates as an Array.
    assert_eq!(sub.classname(&mrb), "MyAry");
    let ary = Array::from_value(sub).expect("subclass instance carries MRB_TT_ARRAY");
    ary.push(&mrb, mrb.str_new(b"x").as_value())
        .expect("push to a fresh array succeeds");
    assert_eq!(ary.entry(0).to_string(&mrb), "x");
}

#[test]
fn class_family_downcasts_agree_with_their_predicates() {
    let mrb = open_mrb();
    let cxt = beni::Ccontext::new(&mrb, c"convert_test.rb")
        .expect("allocating the compile context must succeed");

    for source in [
        &b"String"[..],
        b"'beni'.singleton_class",
        b"Kernel",
        b"'beni'",
        b"42",
        b"nil",
    ] {
        let value = cxt
            .load_nstring(source)
            .expect("the test source must compile and run");
        assert!(
            mrb.pending_exc().is_nil(),
            "evaluating the source must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );

        // The class handle converts on the class and the singleton-class
        // tags, the module handle on the module tag, and on nothing else.
        assert_eq!(
            RClass::from_value(value).is_some(),
            value.is_class() || value.is_sclass(),
            "{}",
            String::from_utf8_lossy(source)
        );
        assert_eq!(
            RModule::from_value(value).is_some(),
            value.is_module(),
            "{}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn every_class_family_handle_round_trips_through_its_value() {
    let mrb = open_mrb();

    let class = mrb.class_get(c"String").expect("String is a core class");
    let singleton = mrb
        .str_new(b"beni")
        .as_value()
        .singleton_class(&mrb)
        .expect("an ordinary object has a singleton class");
    let module = mrb.module_get(c"Kernel").expect("Kernel is a core module");

    for handle in [class, singleton] {
        let back = RClass::from_value(handle.into_value(&mrb))
            .expect("a class handle's value converts back");
        assert_eq!(back.as_raw(), handle.as_raw());
    }
    let back = RModule::from_value(module.into_value(&mrb))
        .expect("a module handle's value converts back");
    assert_eq!(back.as_raw(), module.as_raw());

    // A module is never a class, nor a class a module.
    assert!(RClass::from_value(module.into_value(&mrb)).is_none());
    assert!(RModule::from_value(class.into_value(&mrb)).is_none());
}
