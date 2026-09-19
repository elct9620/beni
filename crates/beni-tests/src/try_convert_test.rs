//! `TryConvert`, the conversion a method's arguments cross: what each
//! target converts, and the exception — class and wording mruby's own —
//! every other value surfaces.

use crate::support::open_mrb;
use beni::{
    Array, Error, ExceptionClass, Hash, Mrb, Proc, RClass, RModule, RString, Range, Symbol,
    TryConvert, Value,
};
use core::num::{NonZeroI32, NonZeroU8};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

fn eval(mrb: &Mrb, source: &str) -> Value {
    mrb.load_string(source.as_bytes())
        .unwrap_or_else(|err| panic!("{source} raised: {}", err.message(mrb)))
}

fn convert<T: TryConvert>(mrb: &Mrb, source: &str) -> T {
    T::try_convert(eval(mrb, source), mrb)
        .unwrap_or_else(|err| panic!("{source} failed to convert: {}", err.message(mrb)))
}

/// The class name and message of the exception converting `source`
/// into `T` surfaces.
fn rejection<T: TryConvert>(mrb: &Mrb, source: &str) -> (String, String) {
    match T::try_convert(eval(mrb, source), mrb) {
        Err(Error::Exception(exc)) => (exc.classname(mrb), Error::Exception(exc).message(mrb)),
        Err(other) => panic!("{source} surfaced a non-exception error: {other:?}"),
        Ok(_) => panic!("{source} must not convert"),
    }
}

fn pair(class: &str, message: &str) -> (String, String) {
    (class.to_owned(), message.to_owned())
}

#[test]
fn an_integer_target_takes_an_integer_and_truncates_a_float() {
    let mrb = open_mrb();
    assert_eq!(convert::<i32>(&mrb, "42"), 42);
    assert_eq!(convert::<i32>(&mrb, "1.9"), 1);
    assert_eq!(convert::<i64>(&mrb, "-2.5"), -2);
    assert_eq!(convert::<i128>(&mrb, "7"), 7);
    assert_eq!(convert::<usize>(&mrb, "3"), 3);
}

#[test]
fn an_integer_outside_the_target_is_a_range_error() {
    let mrb = open_mrb();
    assert_eq!(
        rejection::<u8>(&mrb, "300"),
        pair("RangeError", "300 out of range")
    );
    assert_eq!(
        rejection::<u128>(&mrb, "-1"),
        pair("RangeError", "-1 out of range")
    );
    assert_eq!(rejection::<i32>(&mrb, "(0.0/0.0)").0, "RangeError");
}

#[test]
fn a_non_numeric_integer_argument_is_a_type_error() {
    let mrb = open_mrb();
    assert_eq!(
        rejection::<i32>(&mrb, "'x'"),
        pair("TypeError", "String cannot be converted to Integer")
    );
    assert_eq!(
        rejection::<i32>(&mrb, "nil"),
        pair("TypeError", "nil cannot be converted to Integer")
    );
}

#[test]
fn a_non_zero_target_rejects_zero() {
    let mrb = open_mrb();
    assert_eq!(convert::<NonZeroI32>(&mrb, "5").get(), 5);
    assert_eq!(
        rejection::<NonZeroU8>(&mrb, "0"),
        pair("ArgumentError", "value must be non-zero")
    );
}

#[test]
fn a_float_target_takes_a_float_and_widens_an_integer() {
    let mrb = open_mrb();
    assert_eq!(convert::<f64>(&mrb, "1.5"), 1.5);
    assert_eq!(convert::<f64>(&mrb, "3"), 3.0);
    assert_eq!(convert::<f32>(&mrb, "0.25"), 0.25);
    assert_eq!(
        rejection::<f64>(&mrb, "nil"),
        pair("TypeError", "can't convert nil into Float")
    );
    assert_eq!(
        rejection::<f64>(&mrb, "'x'"),
        pair("TypeError", "String cannot be converted to Float")
    );
}

#[test]
fn value_bool_and_option_convert_as_their_downcasts_do() {
    let mrb = open_mrb();
    assert!(convert::<Value>(&mrb, "nil").is_nil());
    assert!(!convert::<bool>(&mrb, "false"));
    assert!(convert::<bool>(&mrb, "0"));
    assert_eq!(convert::<Option<i32>>(&mrb, "nil"), None);
    assert_eq!(convert::<Option<i32>>(&mrb, "4"), Some(4));
    assert_eq!(rejection::<Option<i32>>(&mrb, "'x'").0, "TypeError");
}

#[test]
fn a_handle_converts_its_own_tag_and_dispatches_no_conversion() {
    let mrb = open_mrb();
    convert::<RString>(&mrb, "'s'");
    convert::<Array>(&mrb, "[]");
    convert::<Hash>(&mrb, "{}");
    convert::<Symbol>(&mrb, ":s");
    convert::<Range>(&mrb, "1..2");
    convert::<Proc>(&mrb, "proc {}");
    assert_eq!(
        rejection::<RString>(&mrb, "o = Object.new; def o.to_str; 's'; end; o"),
        pair("TypeError", "Object cannot be converted to String")
    );
    assert_eq!(
        rejection::<Array>(&mrb, "1"),
        pair("TypeError", "Integer cannot be converted to Array")
    );
    assert_eq!(
        rejection::<Hash>(&mrb, "true"),
        pair("TypeError", "true cannot be converted to Hash")
    );
    assert_eq!(
        rejection::<Symbol>(&mrb, "'s'"),
        pair("TypeError", "String cannot be converted to Symbol")
    );
    assert_eq!(
        rejection::<Range>(&mrb, "[]"),
        pair("TypeError", "Array cannot be converted to Range")
    );
}

#[test]
fn a_proc_target_runs_no_to_proc() {
    let mrb = open_mrb();
    assert_eq!(
        rejection::<Proc>(&mrb, ":upcase"),
        pair("TypeError", "wrong argument type Symbol (expected Proc)")
    );
}

#[test]
fn a_class_or_module_target_words_the_mismatch_as_mruby_does() {
    let mrb = open_mrb();
    convert::<RClass>(&mrb, "String");
    convert::<RModule>(&mrb, "Kernel");
    convert::<ExceptionClass>(&mrb, "RuntimeError");
    assert_eq!(
        rejection::<RClass>(&mrb, "1"),
        pair("TypeError", "1 is not a class")
    );
    assert_eq!(
        rejection::<RModule>(&mrb, "String"),
        pair("TypeError", "String is not a module")
    );
    assert_eq!(
        rejection::<ExceptionClass>(&mrb, "String"),
        pair("TypeError", "String is not a class inheriting Exception")
    );
}

#[test]
fn a_text_target_reads_a_string_and_requires_utf8() {
    let mrb = open_mrb();
    assert_eq!(convert::<String>(&mrb, "'héllo'"), "héllo");
    assert_eq!(convert::<char>(&mrb, "'é'"), 'é');
    assert_eq!(convert::<PathBuf>(&mrb, "'a/b'"), PathBuf::from("a/b"));
    assert_eq!(
        rejection::<String>(&mrb, "\"\\xff\""),
        pair("ArgumentError", "invalid UTF-8 byte sequence")
    );
    assert_eq!(
        rejection::<char>(&mrb, "'ab'"),
        pair("TypeError", "String cannot be converted to char")
    );
    assert_eq!(
        rejection::<String>(&mrb, ":s"),
        pair("TypeError", "Symbol cannot be converted to String")
    );
    assert_eq!(
        rejection::<PathBuf>(&mrb, "1"),
        pair("TypeError", "Integer cannot be converted to String")
    );
}

#[test]
fn a_sequence_target_converts_each_element_of_an_array() {
    let mrb = open_mrb();
    assert_eq!(convert::<Vec<i32>>(&mrb, "[1, 2.5, 3]"), vec![1, 2, 3]);
    assert_eq!(convert::<[String; 2]>(&mrb, "['a', 'b']"), ["a", "b"]);
    assert_eq!(
        convert::<(i32, String)>(&mrb, "[1, 'x']"),
        (1, "x".to_owned())
    );
    let held = convert::<Vec<Value>>(&mrb, "[nil, :s]");
    assert_eq!(held.len(), 2);
    assert_eq!(
        rejection::<Vec<i32>>(&mrb, "[1, 'x']"),
        pair("TypeError", "String cannot be converted to Integer")
    );
    assert_eq!(
        rejection::<(i32, i32)>(&mrb, "[1]"),
        pair("TypeError", "expected Array of length 2")
    );
    assert_eq!(
        rejection::<[i32; 1]>(&mrb, "[1, 2]"),
        pair("TypeError", "expected Array of length 1")
    );
    assert_eq!(
        rejection::<Vec<i32>>(&mrb, "{}"),
        pair("TypeError", "Hash cannot be converted to Array")
    );
}

#[test]
fn a_map_target_converts_each_pair_of_a_hash() {
    let mrb = open_mrb();
    let map = convert::<HashMap<String, i32>>(&mrb, "{'a' => 1, 'b' => 2.9}");
    assert_eq!(
        map,
        HashMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)])
    );
    let ordered = convert::<BTreeMap<i32, bool>>(&mrb, "{2 => nil, 1 => 0}");
    assert_eq!(ordered, BTreeMap::from([(1, true), (2, false)]));
    assert_eq!(
        rejection::<HashMap<String, i32>>(&mrb, "{'a' => 'x'}"),
        pair("TypeError", "String cannot be converted to Integer")
    );
    assert_eq!(
        rejection::<BTreeMap<i32, i32>>(&mrb, "[]"),
        pair("TypeError", "Array cannot be converted to Hash")
    );
}

#[test]
fn the_handles_read_their_elements_into_rust_collections() {
    let mrb = open_mrb();
    let ary = convert::<Array>(&mrb, "[1, 2]");
    assert_eq!(ary.to_vec::<u8>(&mrb).expect("both fit a u8"), vec![1, 2]);
    assert_eq!(
        ary.to_array::<i64, 2>(&mrb).expect("the length matches"),
        [1, 2]
    );
    assert!(ary.to_array::<i64, 3>(&mrb).is_err());
    let hash = convert::<Hash>(&mrb, "{a: 1}");
    assert_eq!(
        hash.to_btree_map::<String, i32>(&mrb)
            .map_err(|e| e.message(&mrb)),
        Err("Symbol cannot be converted to String".to_owned())
    );
    let hash = convert::<Hash>(&mrb, "{'a' => 1}");
    assert_eq!(
        hash.to_hash_map::<String, i32>(&mrb)
            .expect("the pairs convert"),
        HashMap::from([("a".to_owned(), 1)])
    );
}
