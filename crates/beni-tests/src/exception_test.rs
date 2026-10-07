use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Error, ExceptionClass, Mrb};

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
