use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Error, ExceptionClass, Mrb, TryConvert};

type Lookup = fn(&Mrb) -> Result<ExceptionClass, Error>;

fn name(mrb: &Mrb, class: ExceptionClass) -> String {
    class.as_value().inspect(mrb)
}

#[test]
fn each_core_accessor_answers_the_class_of_its_name() {
    let mrb = open_mrb();
    let lookups: [(Lookup, &str); 20] = [
        (Mrb::exception_arg_error, "ArgumentError"),
        (Mrb::exception_float_domain_error, "FloatDomainError"),
        (Mrb::exception_frozen_error, "FrozenError"),
        (Mrb::exception_index_error, "IndexError"),
        (Mrb::exception_key_error, "KeyError"),
        (Mrb::exception_local_jump_error, "LocalJumpError"),
        (Mrb::exception_name_error, "NameError"),
        (
            Mrb::exception_no_matching_pattern_error,
            "NoMatchingPatternError",
        ),
        (Mrb::exception_no_mem_error, "NoMemoryError"),
        (Mrb::exception_no_method_error, "NoMethodError"),
        (Mrb::exception_not_imp_error, "NotImplementedError"),
        (Mrb::exception_range_error, "RangeError"),
        (Mrb::exception_regexp_error, "RegexpError"),
        (Mrb::exception_runtime_error, "RuntimeError"),
        (Mrb::exception_script_error, "ScriptError"),
        (Mrb::exception_stop_iteration, "StopIteration"),
        (Mrb::exception_syntax_error, "SyntaxError"),
        (Mrb::exception_sys_stack_error, "SystemStackError"),
        (Mrb::exception_type_error, "TypeError"),
        (Mrb::exception_zero_div_error, "ZeroDivisionError"),
    ];

    assert_eq!(name(&mrb, mrb.exception_exception()), "Exception");
    assert_eq!(name(&mrb, mrb.exception_standard_error()), "StandardError");
    for (lookup, expected) in lookups {
        let class =
            lookup(&mrb).unwrap_or_else(|err| panic!("{expected} is a core class, got {err}"));
        assert_eq!(name(&mrb, class), expected);
    }
}

#[test]
fn a_looked_up_accessor_follows_its_constant() {
    let mrb = open_mrb();
    let original = mrb.exception_key_error().expect("KeyError is a core class");

    mrb.load_string(b"Object.send(:remove_const, :KeyError)")
        .expect("removing the constant succeeds");
    assert!(
        mrb.exception_key_error().is_err(),
        "a removed constant answers the lookup's Err"
    );

    mrb.load_string(b"KeyError = Class.new(StandardError)")
        .expect("rebinding the constant succeeds");
    let rebound = mrb
        .exception_key_error()
        .expect("the rebound constant is an exception class");
    assert!(!rebound.as_value().is_equal(&mrb, original.as_value()));
}

#[test]
fn the_interpreter_held_accessors_ignore_their_constants() {
    let mrb = open_mrb();
    let exception = mrb.exception_exception();
    let standard_error = mrb.exception_standard_error();

    mrb.load_string(
        b"Object.send(:remove_const, :Exception); Object.send(:remove_const, :StandardError)",
    )
    .expect("removing the constants succeeds");

    assert!(mrb
        .exception_exception()
        .as_value()
        .is_equal(&mrb, exception.as_value()));
    assert!(mrb
        .exception_standard_error()
        .as_value()
        .is_equal(&mrb, standard_error.as_value()));
}

fn takes_array(_mrb: &Mrb, _self: beni::Value, array: beni::RArray) -> i32 {
    array.len() as i32
}

fn panics(_mrb: &Mrb, _self: beni::Value) -> beni::Value {
    panic!("raised as a RuntimeError");
}

/// An interpreter whose `Object` answers `takes_array`, converting its
/// argument to an Array, and `panics`.
fn interpreter_raising_its_own() -> Mrb {
    let mrb = open_mrb();
    let object = mrb.object_class();
    object
        .define_method(&mrb, c"takes_array", beni::method!(takes_array, 1))
        .expect("registering the converting method must succeed");
    object
        .define_method(&mrb, c"panics", beni::method!(panics, 0))
        .expect("registering the panicking method must succeed");
    mrb
}

/// The class name and message of the exception `source` rescues.
fn rescued(mrb: &Mrb, source: &str) -> String {
    let rescuing =
        format!("begin; {source}; rescue Exception => e; \"#{{e.class}}: #{{e.message}}\"; end");
    let answered = mrb
        .load_string(rescuing.as_bytes())
        .unwrap_or_else(|err| panic!("{source} escaped its rescue: {}", err.message(mrb)));
    String::try_convert(answered, mrb).expect("the rescue answers a string")
}

#[test]
fn a_conversion_exception_names_the_class_its_constant_is_rebound_to() {
    let mrb = interpreter_raising_its_own();
    mrb.load_string(
        b"class Rebound < StandardError; end; Object.send(:remove_const, :TypeError); TypeError = Rebound",
    )
    .expect("rebinding the constant succeeds");

    assert_eq!(
        rescued(&mrb, "takes_array(1)"),
        "Rebound: Integer cannot be converted to Array"
    );
}

#[test]
fn a_conversion_exception_whose_constant_is_removed_raises_what_mruby_raises() {
    let mrb = interpreter_raising_its_own();
    mrb.load_string(b"Object.send(:remove_const, :TypeError)")
        .expect("removing the constant succeeds");

    assert_eq!(
        rescued(&mrb, "takes_array(1)"),
        rescued(&mrb, "[].concat(1)"),
        "beni's own TypeError and mruby's own surface the same exception"
    );
    assert_eq!(
        rescued(&mrb, "takes_array(1)"),
        "Exception: exception corrupted"
    );
}

#[test]
fn a_conversion_outside_any_call_surfaces_the_lookup_exception_as_err() {
    let mrb = open_mrb();
    let one = mrb
        .load_string(b"Object.send(:remove_const, :TypeError); 1")
        .expect("removing the constant succeeds");

    let Err(err) = beni::RArray::try_convert(one, &mrb) else {
        panic!("an Integer is no Array");
    };

    assert!(err.is_kind_of(&mrb, mrb.exception_exception()));
    assert_eq!(err.message(&mrb), "exception corrupted");
}

#[test]
fn naming_a_removed_class_runs_no_const_missing() {
    let mrb = interpreter_raising_its_own();
    mrb.load_string(
        b"$missing = nil; def Object.const_missing(name); $missing = name; raise 'hooked'; end; \
          Object.send(:remove_const, :TypeError)",
    )
    .expect("installing the hook succeeds");

    assert_eq!(
        rescued(&mrb, "takes_array(1)"),
        "Exception: exception corrupted"
    );
    let missing = mrb
        .load_string(b"$missing")
        .expect("reading the global succeeds");
    assert!(
        missing.is_nil(),
        "const_missing ran for {}",
        missing.inspect(&mrb)
    );
}

#[test]
fn a_panic_whose_runtime_error_is_removed_raises_what_mruby_raises() {
    let mrb = interpreter_raising_its_own();
    mrb.load_string(b"Object.send(:remove_const, :RuntimeError)")
        .expect("removing the constant succeeds");

    assert_eq!(rescued(&mrb, "panics"), "Exception: exception corrupted");
}
