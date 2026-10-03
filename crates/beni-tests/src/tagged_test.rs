//! Type discrimination through the handles a value converts into by its
//! tag alone: each accepts exactly the values its tag carries.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::{
    Exception, Fiber, Float, Integer, Mrb, Qfalse, Qnil, Qtrue, Qundef, RComplex, RObject,
    RRational, RSet, RStruct, Value,
};

fn eval(mrb: &Mrb, source: &str) -> Value {
    mrb.load_string(source.as_bytes())
        .unwrap_or_else(|err| panic!("{source} raised: {}", err.message(mrb)))
}

#[test]
fn nil_true_and_false_each_convert_only_into_their_own_handle() {
    let mrb = open_mrb();
    let (nil, t, f) = (Value::nil(), eval(&mrb, "true"), eval(&mrb, "false"));

    assert!(Qnil::from_value(nil).is_some());
    assert!(Qtrue::from_value(t).is_some());
    assert!(Qfalse::from_value(f).is_some());

    // nil and false share a tag; only the value tells them apart.
    assert!(Qfalse::from_value(nil).is_none());
    assert!(Qnil::from_value(f).is_none());
    assert!(Qtrue::from_value(nil).is_none());
    assert!(Qnil::from_value(eval(&mrb, "0")).is_none());
}

#[test]
fn the_undefined_value_converts_into_qundef_and_nil_does_not() {
    let mrb = open_mrb();
    // SAFETY: boxing the undefined sentinel reads nothing.
    let undef =
        unsafe { <Value as beni::sys::FromRawValue>::from_raw(beni::sys::mrb_undef_value_func()) };

    assert!(Qundef::from_value(undef).is_some());
    assert!(Qundef::from_value(Value::nil()).is_none());
    assert!(Qundef::from_value(eval(&mrb, "false")).is_none());
}

#[test]
fn numbers_convert_into_the_handle_of_their_own_type() {
    let mrb = open_mrb();
    let (int, float) = (eval(&mrb, "42"), eval(&mrb, "1.5"));

    assert!(Integer::from_value(int).is_some());
    assert!(Float::from_value(float).is_some());
    assert!(Integer::from_value(float).is_none());
    assert!(Float::from_value(int).is_none());
    assert!(Integer::from_value(eval(&mrb, "'42'")).is_none());
}

#[test]
fn an_ordinary_object_converts_into_robject_and_a_built_in_layout_does_not() {
    let mrb = open_mrb();
    let object = eval(&mrb, "class BeniPlain; end; BeniPlain.new");

    assert!(RObject::from_value(object).is_some());
    assert!(RObject::from_value(eval(&mrb, "Object.new")).is_some());
    assert!(RObject::from_value(eval(&mrb, "'s'")).is_none());
    assert!(RObject::from_value(eval(&mrb, "RuntimeError.new")).is_none());
    assert!(RObject::from_value(Value::nil()).is_none());
}

#[test]
fn an_exception_object_converts_into_exception_and_its_class_does_not() {
    let mrb = open_mrb();

    assert!(Exception::from_value(eval(&mrb, "RuntimeError.new('x')")).is_some());
    assert!(Exception::from_value(eval(&mrb, "RuntimeError")).is_none());
    assert!(Exception::from_value(eval(&mrb, "Object.new")).is_none());
}

#[test]
fn a_gem_defined_object_converts_into_the_handle_of_its_own_tag() {
    let mrb = open_mrb();
    let fiber = eval(&mrb, "Fiber.new { 1 }");
    let strukt = eval(&mrb, "Struct.new(:a).new(1)");
    let set = eval(&mrb, "Set.new([1])");
    let complex = eval(&mrb, "Complex(1, 2)");

    assert!(Fiber::from_value(fiber).is_some());
    assert!(RStruct::from_value(strukt).is_some());
    assert!(RSet::from_value(set).is_some());
    assert!(RComplex::from_value(complex).is_some());

    // Each rejects the others, and an ordinary object.
    assert!(Fiber::from_value(strukt).is_none());
    assert!(RStruct::from_value(set).is_none());
    assert!(RSet::from_value(complex).is_none());
    assert!(RComplex::from_value(fiber).is_none());
    assert!(RRational::from_value(complex).is_none());
    assert!(RObject::from_value(strukt).is_none());
}

#[test]
fn a_tagged_handle_answers_the_value_it_was_converted_from() {
    let mrb = open_mrb();
    let object = eval(&mrb, "Object.new");
    let handle = RObject::from_value(object).expect("an Object converts");

    assert!(crate::support::same_object(&mrb, handle, object));
    assert!(handle.as_value().obj_equal(&mrb, object));
}

#[test]
fn a_class_handle_tells_a_singleton_class_from_an_ordinary_one() {
    let mrb = open_mrb();
    let singleton = beni::RClass::from_value(eval(&mrb, "'s'.singleton_class"))
        .expect("a singleton class converts into a class handle");
    let ordinary = beni::RClass::from_value(eval(&mrb, "String")).expect("a class converts");

    assert!(singleton.is_singleton());
    assert!(!ordinary.is_singleton());
}
