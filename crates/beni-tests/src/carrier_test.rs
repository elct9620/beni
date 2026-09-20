//! `Mrb::mark_carrier` / `Mrb::carrier`: the record of which class each
//! `TypedData` class path was marked as, and what a Ruby program can
//! do to it.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::{DataType, Mrb, RClass, TryConvert, TypedData};

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

struct HandWritten;
static HAND_WRITTEN: DataType<HandWritten> = DataType::new(c"BeniCarrierHandWritten");

// SAFETY: `mark_carriers` marks the class `class` answers, and no
// value wraps before the test calls it.
unsafe impl TypedData for HandWritten {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniCarrierHandWritten")
            .expect("the test defines it before naming it")
    }

    fn data_type() -> &'static DataType<Self> {
        &HAND_WRITTEN
    }
}

#[test]
fn a_marked_path_answers_its_class_and_carries_data() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierPoint; end");

    let marked = mrb
        .mark_carrier(c"BeniCarrierPoint")
        .expect("names a class");

    assert!(mrb
        .carrier(c"BeniCarrierPoint")
        .is_some_and(|held| held.as_value().obj_equal(&mrb, marked.as_value())));
    let err = mrb
        .load_string(b"BeniCarrierPoint.new")
        .expect_err("the marked class has its allocator undefined");
    assert_eq!(
        err.message(&mrb),
        "allocator undefined for BeniCarrierPoint"
    );
}

#[test]
fn a_nested_path_resolves_one_constant_per_segment() {
    let mrb = open_mrb();
    define(&mrb, b"module BeniCarrierOuter; class Inner; end; end");

    let marked = mrb
        .mark_carrier(c"BeniCarrierOuter::Inner")
        .expect("names a nested class");

    assert!(marked
        .as_value()
        .obj_equal(&mrb, class(&mrb, "BeniCarrierOuter::Inner").as_value()));
}

#[test]
fn an_unmarked_path_is_held_by_nothing() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierUnmarked; end");

    assert!(mrb.carrier(c"BeniCarrierUnmarked").is_none());
}

#[test]
fn a_path_naming_no_class_holds_nothing() {
    let mrb = open_mrb();
    define(&mrb, b"BeniCarrierNotAClass = 1");

    let missing = mrb
        .mark_carrier(c"BeniCarrierMissing")
        .expect_err("no constant is bound under the path");
    let not_a_class = mrb
        .mark_carrier(c"BeniCarrierNotAClass")
        .expect_err("the constant is not a class");
    let refused = mrb
        .mark_carrier(c"String")
        .expect_err("a string's layout refuses the carrier mark");

    assert!(missing.message(&mrb).contains("BeniCarrierMissing"));
    assert_eq!(not_a_class.message(&mrb), "1 is not a class");
    assert!(refused.message(&mrb).contains("carry Rust data"));
    assert!(mrb.carrier(c"BeniCarrierMissing").is_none());
    assert!(mrb.carrier(c"BeniCarrierNotAClass").is_none());
}

#[test]
fn marking_a_held_path_again_replaces_what_it_holds() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierFirst; end");
    mrb.mark_carrier(c"BeniCarrierFirst")
        .expect("names a class");
    define(
        &mrb,
        b"BeniCarrierOther = Class.new; Object.const_set(:BeniCarrierFirst, BeniCarrierOther)",
    );

    let remarked = mrb
        .mark_carrier(c"BeniCarrierFirst")
        .expect("the path now names the other class");

    assert!(remarked
        .as_value()
        .obj_equal(&mrb, class(&mrb, "BeniCarrierOther").as_value()));
}

#[test]
fn no_ruby_program_reaches_the_record_through_a_global() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierGuarded; end");
    let marked = mrb
        .mark_carrier(c"BeniCarrierGuarded")
        .expect("names a class");

    define(&mrb, b"$beni_carriers = nil; beni_carriers = nil");

    assert!(mrb
        .carrier(c"BeniCarrierGuarded")
        .is_some_and(|held| held.as_value().obj_equal(&mrb, marked.as_value())));
}

#[test]
fn each_interpreter_holds_its_own_record() {
    let first = open_mrb();
    let second = open_mrb();
    define(&first, b"class BeniCarrierPerState; end");
    define(&second, b"class BeniCarrierPerState; end");

    first
        .mark_carrier(c"BeniCarrierPerState")
        .expect("names a class");

    assert!(first.carrier(c"BeniCarrierPerState").is_some());
    assert!(second.carrier(c"BeniCarrierPerState").is_none());
}

#[test]
fn a_hand_written_implementation_marks_the_class_it_names() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierHandWritten; end");

    HandWritten::mark_carriers(&mrb).expect("the class accepts the mark");

    let wrapped = mrb.wrap(HandWritten);
    assert!(wrapped
        .as_value()
        .is_kind_of(&mrb, class(&mrb, "BeniCarrierHandWritten")));
    let err = mrb
        .load_string(b"BeniCarrierHandWritten.new")
        .expect_err("marking undefines the default allocator");
    assert_eq!(
        err.message(&mrb),
        "allocator undefined for BeniCarrierHandWritten"
    );
}

#[test]
fn the_record_keeps_its_class_through_a_collection() {
    let mrb = open_mrb();
    define(&mrb, b"class BeniCarrierHeld; end");
    let marked = mrb.mark_carrier(c"BeniCarrierHeld").expect("names a class");

    define(&mrb, b"Object.const_set(:BeniCarrierHeld, Class.new)");
    mrb.full_gc();

    let held = mrb
        .carrier(c"BeniCarrierHeld")
        .expect("the record still holds the class the constant let go of");
    assert!(held.as_value().obj_equal(&mrb, marked.as_value()));
    assert_eq!(
        held.name(&mrb),
        "BeniCarrierHeld",
        "the class is alive enough to answer its own name"
    );
}
