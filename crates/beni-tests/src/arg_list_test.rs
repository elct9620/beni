use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Error, IntoValue, RArray, RClass, RString, Value};

#[test]
fn a_tuple_converts_each_rust_value_and_the_result_converts_to_the_type_named() {
    let mrb = open_mrb();
    let sum: i64 = 40i32
        .into_value(&mrb)
        .funcall(&mrb, "+", (2i32,))
        .expect("Integer#+ takes an Integer");
    assert_eq!(sum, 42);

    let joined: String = mrb
        .eval::<Value>("[]")
        .expect("the literal evaluates")
        .funcall(&mrb, "push", ("a", 1i32, true))
        .and_then(|ary: RArray| ary.as_value().funcall(&mrb, "inspect", ()))
        .expect("the pushed array inspects");
    assert_eq!(joined, r#"["a", 1, true]"#);
}

#[test]
fn a_slice_of_value_handles_and_a_slice_of_class_handles_both_reach_the_call() {
    let mrb = open_mrb();
    let strings = [mrb.str_new(b"x"), mrb.str_new(b"y")];
    // A class handle holds its class pointer rather than its value, so its
    // slice is copied; no boxing is where reading it in place would show.
    let classes = [mrb.class_string(), mrb.class_array()];
    let ary = mrb.ary_new();

    let _: Value = ary
        .as_value()
        .funcall(&mrb, "push", &strings[..])
        .expect("push takes the strings");
    let _: Value = ary
        .as_value()
        .funcall(&mrb, "push", &classes[..])
        .expect("push takes the classes");

    assert_eq!(ary.entry::<String>(&mrb, 1).expect("a String reads"), "y");
    let class = ary.entry::<RClass>(&mrb, 3).expect("a Class reads");
    assert_eq!(
        class
            .as_value()
            .funcall::<_, _, String>(&mrb, "to_s", ())
            .expect("a class renders"),
        "Array"
    );
    assert_eq!(ary.len(), 4);
    let _: [RString; 2] = strings;
}

#[test]
fn a_trailing_hash_reaches_the_method_as_a_positional_argument() {
    let mrb = open_mrb();
    let receiver: Value = mrb
        .eval("o = Object.new; def o.positional(h) = h.size; def o.keyword(a: 1) = a; o")
        .expect("the receiver defines both methods");
    let hash = mrb.hash_new();
    hash.aset(&mrb, mrb.intern(b"a").expect("interns"), 2i32)
        .expect("assign succeeds");

    let size: i64 = receiver
        .funcall(&mrb, "positional", (hash,))
        .expect("the hash is the one positional");
    assert_eq!(size, 1);

    let err = receiver
        .funcall::<_, _, Value>(&mrb, "keyword", (hash,))
        .expect_err("a positional hash is no keyword");
    let Error::Exception(exc) = err else {
        panic!("the call must raise, got {err}");
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
}
