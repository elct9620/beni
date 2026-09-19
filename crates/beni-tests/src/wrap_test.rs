//! `#[beni::wrap]` / `#[derive(beni::TypedData)]`: what the generated
//! implementation does with the class it names.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Mrb, RClass, TryConvert, TypedData};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[beni::wrap(class = "BeniWrapOuter::Inner")]
struct Nested;

#[beni::wrap(class = "BeniWrapDefaultName")]
struct DefaultName;

#[beni::wrap(class = "BeniWrapDefaultName", name = "BeniWrapCustom")]
struct CustomName;

#[beni::wrap(class = "BeniWrapMissing")]
struct Missing;

#[beni::wrap(class = "String")]
struct Stringly;

#[beni::wrap(class = "BeniWrapBase")]
struct Based;

#[derive(beni::TypedData)]
#[beni(class = "BeniWrapShape")]
enum Shape {
    #[beni(class = "BeniWrapShape::Circle")]
    Circle,
    Square,
}

fn define(mrb: &Mrb, source: &[u8]) {
    mrb.load_string(source)
        .expect("defining the classes must succeed");
}

fn class(mrb: &Mrb, path: &str) -> RClass {
    let value = mrb
        .load_string(path.as_bytes())
        .expect("the class is defined");
    RClass::try_convert(value, mrb).expect("the constant is a class")
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload.downcast::<&str>().map(|m| m.to_string()).unwrap(),
    }
}

#[test]
fn a_nested_path_names_its_class_and_prepares_it_to_carry_data() {
    let mrb = open_mrb();
    define(&mrb, b"module BeniWrapOuter; class Inner; end; end");

    let wrapped = mrb.wrap(Nested);

    let inner = class(&mrb, "BeniWrapOuter::Inner");
    assert!(wrapped.as_value().is_kind_of(&mrb, inner));
    let err = mrb
        .load_string(b"BeniWrapOuter::Inner.new")
        .expect_err("the named class has its allocator undefined");
    assert_eq!(
        err.message(&mrb),
        "allocator undefined for BeniWrapOuter::Inner"
    );
}

#[test]
fn the_data_type_is_named_after_the_class_unless_named_otherwise() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniWrapDefaultName; end");

    let custom = mrb.wrap(CustomName).as_value();
    let default = mrb.wrap(DefaultName).as_value();

    let err = <&DefaultName>::try_convert(custom, &mrb).err().unwrap();
    assert!(err
        .message(&mrb)
        .ends_with("(expected BeniWrapDefaultName)"));
    let err = <&CustomName>::try_convert(default, &mrb).err().unwrap();
    assert!(err.message(&mrb).ends_with("(expected BeniWrapCustom)"));
}

#[test]
fn a_path_naming_no_class_panics() {
    let mrb = open_mrb();
    define(&mrb, b"BeniWrapMissing = 1");

    let payload = catch_unwind(AssertUnwindSafe(|| mrb.wrap(Missing)))
        .err()
        .unwrap();

    assert!(panic_message(payload).starts_with("BeniWrapMissing does not name a class"));
}

#[test]
fn a_class_refusing_the_mark_panics() {
    let mrb = open_mrb();

    let payload = catch_unwind(AssertUnwindSafe(|| mrb.wrap(Stringly)))
        .err()
        .unwrap();

    assert!(panic_message(payload).starts_with("String cannot carry Rust data"));
    assert!(
        mrb.load_string(b"String.new").is_ok(),
        "a refused class keeps its allocator"
    );
}

#[test]
fn each_variant_wraps_as_its_own_class_or_the_types() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniWrapShape; class Circle < self; end; end");

    let circle = mrb.wrap(Shape::Circle).as_value();
    let square = mrb.wrap(Shape::Square).as_value();

    assert_eq!(circle.classname(&mrb), "BeniWrapShape::Circle");
    assert_eq!(square.classname(&mrb), "BeniWrapShape");
}

#[test]
fn a_subclass_defined_before_the_class_is_named_cannot_be_wrapped_into() {
    let mrb = open_mrb();
    define(
        &mrb,
        b"class BeniWrapBase; end; class BeniWrapEarly < BeniWrapBase; end",
    );
    let early = class(&mrb, "BeniWrapEarly");

    let wrapped = catch_unwind(AssertUnwindSafe(|| mrb.wrap_as(Based, early)));

    assert!(wrapped.is_err(), "the earlier subclass took no mark");
}

#[test]
fn a_subclass_defined_after_the_class_is_named_carries_data() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniWrapBase; end");
    Based::class(&mrb);
    define(&mrb, b"class BeniWrapLate < BeniWrapBase; end");
    let late = class(&mrb, "BeniWrapLate");

    let wrapped = mrb.wrap_as(Based, late);

    assert!(wrapped.as_value().is_kind_of(&mrb, late));
}

#[test]
fn each_interpreter_resolves_its_own_class() {
    let first = open_mrb();
    let second = open_mrb();
    define(&first, b"module BeniWrapOuter; class Inner; end; end");
    define(&second, b"module BeniWrapOuter; class Inner; end; end");

    let in_first = first.wrap(Nested);
    let in_second = second.wrap(Nested);

    assert!(in_first
        .as_value()
        .is_kind_of(&first, class(&first, "BeniWrapOuter::Inner")));
    assert!(in_second
        .as_value()
        .is_kind_of(&second, class(&second, "BeniWrapOuter::Inner")));
}
