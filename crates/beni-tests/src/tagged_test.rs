//! Type discrimination through the handles a value converts into by its
//! tag alone: each accepts exactly the values its tag carries.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::TryConvert;
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

/// The class and message of the `Err` converting `source`'s value into `T`.
fn rejection<T: beni::TryConvert>(mrb: &Mrb, source: &str) -> (String, String) {
    match T::try_convert(eval(mrb, source), mrb) {
        Ok(_) => panic!("{source} must not convert"),
        Err(err) => {
            let beni::Error::Exception(exc) = &err else {
                panic!("{source} must surface an exception")
            };
            (exc.classname(mrb), err.message(mrb))
        }
    }
}

fn type_error(message: &str) -> (String, String) {
    ("TypeError".to_owned(), message.to_owned())
}

#[test]
fn a_tagged_handle_takes_an_argument_of_its_own_tag() {
    let mrb = open_mrb();
    let accepts = |source: &str| {
        let value = eval(&mrb, source);
        [
            Qnil::try_convert(value, &mrb).is_ok(),
            Qtrue::try_convert(value, &mrb).is_ok(),
            Qfalse::try_convert(value, &mrb).is_ok(),
            Exception::try_convert(value, &mrb).is_ok(),
            RObject::try_convert(value, &mrb).is_ok(),
            Fiber::try_convert(value, &mrb).is_ok(),
            RStruct::try_convert(value, &mrb).is_ok(),
            RSet::try_convert(value, &mrb).is_ok(),
            RRational::try_convert(value, &mrb).is_ok(),
            RComplex::try_convert(value, &mrb).is_ok(),
        ]
        .iter()
        .position(|ok| *ok)
    };

    assert_eq!(accepts("nil"), Some(0));
    assert_eq!(accepts("true"), Some(1));
    assert_eq!(accepts("false"), Some(2));
    assert_eq!(accepts("RuntimeError.new('boom')"), Some(3));
    assert_eq!(accepts("Object.new"), Some(4));
    assert_eq!(accepts("Fiber.new {}"), Some(5));
    assert_eq!(accepts("Struct.new(:a).new(1)"), Some(6));
    assert_eq!(accepts("Set.new"), Some(7));
    assert_eq!(accepts("Complex(1, 2)"), Some(9));
}

#[test]
fn a_tagged_handle_words_a_mismatch_as_mruby_type_check_does() {
    let mrb = open_mrb();

    assert_eq!(
        rejection::<Qnil>(&mrb, "false"),
        type_error("wrong argument type false (expected NilClass)")
    );
    assert_eq!(
        rejection::<Qtrue>(&mrb, "nil"),
        type_error("wrong argument type nil (expected TrueClass)")
    );
    assert_eq!(
        rejection::<Qfalse>(&mrb, "true"),
        type_error("wrong argument type true (expected FalseClass)")
    );
    assert_eq!(
        rejection::<Exception>(&mrb, "1"),
        type_error("wrong argument type Integer (expected Exception)")
    );
    assert_eq!(
        rejection::<RObject>(&mrb, "'text'"),
        type_error("wrong argument type String (expected Object)")
    );
    assert_eq!(
        rejection::<Fiber>(&mrb, ":sym"),
        type_error("wrong argument type Symbol (expected Fiber)")
    );
    assert_eq!(
        rejection::<RStruct>(&mrb, "[]"),
        type_error("wrong argument type Array (expected Struct)")
    );
    assert_eq!(
        rejection::<RSet>(&mrb, "{}"),
        type_error("wrong argument type Hash (expected Set)")
    );
    assert_eq!(
        rejection::<RRational>(&mrb, "nil"),
        type_error("wrong argument type nil (expected Rational)")
    );
    assert_eq!(
        rejection::<RComplex>(&mrb, "Object.new"),
        type_error("wrong argument type Object (expected Complex)")
    );
    assert_eq!(
        rejection::<beni::RInlineStruct>(&mrb, "Object.new"),
        type_error("wrong argument type Object (expected istruct)")
    );
    assert_eq!(
        rejection::<beni::RCptr>(&mrb, "Object.new"),
        type_error("wrong argument type Object (expected cptr)")
    );
}

#[test]
fn an_exception_argument_dispatches_no_exception_method() {
    let mrb = open_mrb();

    // An object answering `exception` is what `raise` accepts, but the
    // argument conversion runs no Ruby and takes the tag alone.
    let (class, _) = rejection::<Exception>(
        &mrb,
        "o = Object.new; def o.exception(*a); RuntimeError.new('x'); end; o",
    );
    assert_eq!(class, "TypeError");
}

#[test]
fn an_integer_argument_coerces_a_float_as_mruby_does() {
    let mrb = open_mrb();
    let int = |source: &str| {
        let handle = Integer::try_convert(eval(&mrb, source), &mrb)
            .unwrap_or_else(|err| panic!("{source} must convert: {}", err.message(&mrb)));
        i64::from_value(handle.as_value())
    };

    assert_eq!(int("42"), Some(42));
    assert_eq!(int("3.9"), Some(3));
    assert_eq!(int("-3.9"), Some(-3));
    assert_eq!(
        rejection::<Integer>(&mrb, "'abc'"),
        type_error("String cannot be converted to Integer")
    );
    assert_eq!(
        rejection::<Integer>(&mrb, "nil"),
        type_error("nil cannot be converted to Integer")
    );
    assert_eq!(rejection::<Integer>(&mrb, "1.0 / 0").0, "RangeError");
    assert_eq!(rejection::<Integer>(&mrb, "0.0 / 0").0, "RangeError");
}

#[test]
fn a_float_argument_widens_an_integer_as_mruby_does() {
    let mrb = open_mrb();
    let float = |source: &str| {
        let handle = Float::try_convert(eval(&mrb, source), &mrb)
            .unwrap_or_else(|err| panic!("{source} must convert: {}", err.message(&mrb)));
        f64::from_value(handle.as_value())
    };

    assert_eq!(float("1.5"), Some(1.5));
    assert_eq!(float("2"), Some(2.0));
    assert_eq!(
        rejection::<Float>(&mrb, "nil"),
        type_error("can't convert nil into Float")
    );
    assert_eq!(
        rejection::<Float>(&mrb, "'abc'"),
        type_error("String cannot be converted to Float")
    );
}
