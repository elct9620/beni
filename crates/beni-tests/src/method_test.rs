use beni::{Error, FromValue, IntoValue, Mrb, RString, Value};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::support::open_mrb;
use beni::Module;

static TYPE_ERROR_BODY_RAN: AtomicBool = AtomicBool::new(false);

fn add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
    a + b
}

fn observed_add(_mrb: &Mrb, _self: Value, a: i32, b: i32) -> i32 {
    TYPE_ERROR_BODY_RAN.store(true, Ordering::SeqCst);
    a + b
}

fn boom(_mrb: &Mrb, _self: Value) -> Value {
    panic!("boom in registered method");
}

// Optional trailing argument: present binds `Some`, omitted binds
// `None` (defaulting to a base of 0).
fn opt_add(_mrb: &Mrb, _self: Value, a: i32, b: Option<i32>) -> i32 {
    a + b.unwrap_or(0)
}

// No required arguments, one optional — proves the all-optional
// form reads the lone optional slot.
fn opt_only(_mrb: &Mrb, _self: Value, a: Option<i32>) -> i32 {
    a.unwrap_or(-1)
}

// Block-accepting method: yields the required argument to the block
// when one was passed, returning the block's value; with no block
// the slot binds `None` and the body returns the argument unchanged.
fn apply_block(mrb: &Mrb, _self: Value, a: i32, block: Option<beni::Proc>) -> Result<Value, Error> {
    match block {
        Some(b) => b.call(mrb, &[a.into_value(mrb)]),
        None => Ok(a.into_value(mrb)),
    }
}

fn fallible(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
    let runtime_error = mrb
        .exc_get(c"RuntimeError")
        .expect("RuntimeError is a core class");
    Err(Error::Exception(
        runtime_error.exc_new(mrb, "fallible body says no"),
    ))
}

fn fresh_class(mrb: &Mrb, name: &core::ffi::CStr) -> beni::RClass {
    mrb.define_class(name, mrb.object_class())
        .expect("defining the test class must succeed")
}

#[test]
fn typed_method_roundtrips_scalars() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniAdder");
    class
        .define_method(&mrb, c"add", beni::method!(add, 2))
        .expect("registering the typed method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)];
    let got = receiver
        .funcall(&mrb, c"add", &args)
        .expect("the call must not raise");
    assert_eq!(i32::from_value(got), Some(3));
}

#[test]
fn fixed_arity_raises_argument_error_on_wrong_count() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniArityAdder");
    class
        .define_method(&mrb, c"add", beni::method!(add, 2))
        .expect("registering the fixed-arity method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // `mrb_get_args` enforces the `MRB_ARGS_REQ(2)` count before any
    // `FromValue` conversion runs — both too few and too many
    // positionals reach the caller as `ArgumentError`.
    for wrong in [
        vec![Value::from_int(&mrb, 1)],
        vec![
            Value::from_int(&mrb, 1),
            Value::from_int(&mrb, 2),
            Value::from_int(&mrb, 3),
        ],
    ] {
        let err = receiver
            .funcall(&mrb, c"add", &wrong)
            .expect_err("a wrong argument count must surface as Err");
        match err {
            Error::Exception(exc) => assert_eq!(exc.classname(&mrb), "ArgumentError"),
            other => panic!("a wrong argument count must raise, not panic, got {other}"),
        }
    }

    // The count error left the VM intact: a correctly-counted
    // call still dispatches and returns.
    let got = receiver
        .funcall(
            &mrb,
            c"add",
            &[Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)],
        )
        .expect("the VM survives the count error and the next call runs");
    assert_eq!(i32::from_value(got), Some(3));
}

#[test]
fn from_value_failure_raises_before_body_runs() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniStrictAdder");
    class
        .define_method(&mrb, c"add", beni::method!(observed_add, 2))
        .expect("registering the typed method must succeed");

    // A Float argument fails the i32 FromValue conversion: the
    // bridge must raise TypeError to the Ruby caller and the
    // wrapped function must never run.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [Value::from_float(&mrb, 1.5), Value::from_int(&mrb, 2)];
    let err = receiver
        .funcall(&mrb, c"add", &args)
        .expect_err("the conversion failure must surface as a raise");
    assert!(
        err.message(&mrb).contains("i32"),
        "the TypeError must name the expected Rust type: {}",
        err.message(&mrb)
    );
    assert!(
        !TYPE_ERROR_BODY_RAN.load(Ordering::SeqCst),
        "the wrapped function must not run on conversion failure"
    );
}

#[test]
fn optional_argument_defaults_to_none_when_omitted() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniOptAdder");
    class
        .define_method(&mrb, c"add", beni::method!(opt_add, 1, 1))
        .expect("registering the optional-arg method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // Omitting the optional argument binds `None`: the body sees
    // the default base of 0.
    let omitted = receiver
        .funcall(&mrb, c"add", &[Value::from_int(&mrb, 7)])
        .expect("the call with the optional omitted must not raise");
    assert_eq!(i32::from_value(omitted), Some(7));

    // Supplying it binds `Some`.
    let supplied = receiver
        .funcall(
            &mrb,
            c"add",
            &[Value::from_int(&mrb, 7), Value::from_int(&mrb, 5)],
        )
        .expect("the call with the optional supplied must not raise");
    assert_eq!(i32::from_value(supplied), Some(12));
}

#[test]
fn all_optional_method_reads_its_lone_slot() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniOptOnly");
    class
        .define_method(&mrb, c"v", beni::method!(opt_only, 0, 1))
        .expect("registering the all-optional method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    let omitted = receiver
        .funcall(&mrb, c"v", &[])
        .expect("the call with no arguments must not raise");
    assert_eq!(i32::from_value(omitted), Some(-1));

    let supplied = receiver
        .funcall(&mrb, c"v", &[Value::from_int(&mrb, 42)])
        .expect("the call with the optional supplied must not raise");
    assert_eq!(i32::from_value(supplied), Some(42));
}

#[test]
fn supplied_optional_failing_from_value_raises() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniOptStrict");
    class
        .define_method(&mrb, c"add", beni::method!(opt_add, 1, 1))
        .expect("registering the optional-arg method must succeed");

    // A supplied optional that fails its i32 conversion raises, just
    // as a required argument does — an omitted slot would bind None
    // instead, so the raise is specific to the supplied case.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let err = receiver
        .funcall(
            &mrb,
            c"add",
            &[Value::from_int(&mrb, 1), Value::from_float(&mrb, 1.5)],
        )
        .expect_err("the supplied optional's conversion failure must raise");
    assert!(
        err.message(&mrb).contains("i32"),
        "the TypeError must name the expected Rust type: {}",
        err.message(&mrb)
    );
}

#[test]
fn block_accepting_method_yields_to_a_passed_block() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniBlockApply");
    class
        .define_method(&mrb, c"apply", beni::method!(apply_block, 1, &))
        .expect("registering the block-accepting method must succeed");

    // Calling with a block: the registered method receives it as
    // `Some(Proc)` and yields the argument to it, here doubling it.
    let cxt = beni::Ccontext::new(&mrb, c"block_method_test.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(b"BeniBlockApply.new.apply(21) { |x| x * 2 }")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "the block-yielding call must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn block_accepting_method_binds_none_without_a_block() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniBlockOptional");
    class
        .define_method(&mrb, c"apply", beni::method!(apply_block, 1, &))
        .expect("registering the block-accepting method must succeed");

    // No block passed: the slot is nil, the parameter binds `None`,
    // and the body returns the argument unchanged.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"apply", &[Value::from_int(&mrb, 7)])
        .expect("the call without a block must not raise");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn panicking_method_surfaces_as_ruby_exception() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniPanicker");
    class
        .define_method(&mrb, c"detonate", beni::method!(boom, 0))
        .expect("registering the panicking method must succeed");

    // The panic must be caught at the bridge and re-raised as a
    // RuntimeError the Ruby-side caller (here: the protect frame)
    // observes — never an unwind through mruby's C frames.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let err = receiver
        .funcall(&mrb, c"detonate", &[])
        .expect_err("the panic must surface as a Ruby exception");
    assert!(matches!(err, Error::Exception(_)));
    assert!(
        err.message(&mrb).contains("boom in registered method"),
        "the RuntimeError must carry the panic message: {}",
        err.message(&mrb)
    );
}

#[test]
fn protect_surfaces_closure_panic_as_err() {
    let mrb = open_mrb();

    let err = mrb
        .protect(|_| panic!("pop goes the closure"))
        .expect_err("the panic must surface as Err");
    match err {
        Error::Panic(msg) => assert!(msg.contains("pop goes the closure")),
        other => panic!("a closure panic must surface as Error::Panic, got {other}"),
    }
}

#[test]
fn result_returning_method_raises_its_err() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniFallible");
    class
        .define_method(&mrb, c"try", beni::method!(fallible, 0))
        .expect("registering the fallible method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let err = receiver
        .funcall(&mrb, c"try", &[])
        .expect_err("the body's Err must surface as a raise");
    assert!(
        err.message(&mrb).contains("fallible body says no"),
        "the raised exception must be the body's own: {}",
        err.message(&mrb)
    );
}

fn echo_string(_mrb: &Mrb, _self: Value, v: beni::RString) -> beni::RString {
    v
}

fn echo_array(_mrb: &Mrb, _self: Value, v: beni::Array) -> beni::Array {
    v
}

fn echo_hash(_mrb: &Mrb, _self: Value, v: beni::Hash) -> beni::Hash {
    v
}

fn echo_proc(_mrb: &Mrb, _self: Value, v: beni::Proc) -> beni::Proc {
    v
}

fn echo_range(_mrb: &Mrb, _self: Value, v: beni::Range) -> beni::Range {
    v
}

fn echo_class(_mrb: &Mrb, _self: Value, v: beni::RClass) -> beni::RClass {
    v
}

fn kernel_module(mrb: &Mrb, _self: Value) -> beni::RModule {
    mrb.module_get(c"Kernel").expect("Kernel is a core module")
}

#[test]
fn a_returned_handle_reaches_ruby_as_the_object_it_names() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniHandleEcho");
    for (name, method) in [
        (c"string", beni::method!(echo_string, 1)),
        (c"array", beni::method!(echo_array, 1)),
        (c"hash", beni::method!(echo_hash, 1)),
        (c"proc", beni::method!(echo_proc, 1)),
        (c"range", beni::method!(echo_range, 1)),
        (c"klass", beni::method!(echo_class, 1)),
        (c"kernel", beni::method!(kernel_module, 0)),
    ] {
        class
            .define_method(&mrb, name, method)
            .expect("registering the handle-returning method must succeed");
    }

    let cxt = beni::Ccontext::new(&mrb, c"handle_echo_test.rb")
        .expect("allocating the compile context must succeed");
    for probe in [
        "s = 'beni'; BeniHandleEcho.new.string(s).equal?(s)",
        "a = [1]; BeniHandleEcho.new.array(a).equal?(a)",
        "h = { a: 1 }; BeniHandleEcho.new.hash(h).equal?(h)",
        "pr = proc {}; BeniHandleEcho.new.proc(pr).equal?(pr)",
        "r = (1..2); BeniHandleEcho.new.range(r).equal?(r)",
        "BeniHandleEcho.new.klass(String).equal?(String)",
        "BeniHandleEcho.new.kernel.equal?(Kernel)",
    ] {
        let got = cxt
            .load_nstring(probe.as_bytes())
            .expect("the probe must compile and run");
        assert!(
            mrb.pending_exc().is_nil(),
            "`{probe}` must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert!(got.is_true(), "`{probe}` must answer the same object");
    }
}

// A reporter whose location parts may each be nil: renders what it was
// given, with `-` for an absent part.
fn report(
    mrb: &Mrb,
    _self: Value,
    message: String,
    file: Option<String>,
    line: Option<i64>,
) -> RString {
    let file = file.unwrap_or_else(|| "-".to_owned());
    let line = line.map_or_else(|| "-".to_owned(), |l| l.to_string());
    mrb.str_new(format!("{message}@{file}:{line}").as_bytes())
}

fn passthrough(_mrb: &Mrb, _self: Value, value: Value) -> Value {
    value
}

// Tells the three states of a nilable optional apart: omitted, given
// nil, and given an Integer.
fn nilable_optional(_mrb: &Mrb, _self: Value, given: Option<Option<i64>>) -> i32 {
    match given {
        None => 0,
        Some(None) => 1,
        Some(Some(_)) => 2,
    }
}

fn rendered(mrb: &Mrb, value: Value) -> String {
    String::from_value(value)
        .unwrap_or_else(|| panic!("expected a String, got {}", value.classname(mrb)))
}

#[test]
fn nilable_parameters_bind_none_for_nil_and_the_value_otherwise() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniReporter");
    class
        .define_method(&mrb, c"error", beni::method!(report, 3))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");
    let message = mrb.str_new(b"boom").as_value();

    let absent = receiver
        .funcall(&mrb, c"error", &[message, Value::nil(), Value::nil()])
        .expect("nil is accepted for a nilable parameter");
    let present = receiver
        .funcall(
            &mrb,
            c"error",
            &[
                message,
                mrb.str_new(b"a.rb").as_value(),
                Value::from_int(&mrb, 42),
            ],
        )
        .expect("the inner type is accepted for a nilable parameter");

    assert_eq!(rendered(&mrb, absent), "boom@-:-");
    assert_eq!(rendered(&mrb, present), "boom@a.rb:42");
}

#[test]
fn a_nilable_parameter_still_rejects_what_its_inner_type_rejects() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniStrictReporter");
    class
        .define_method(&mrb, c"error", beni::method!(report, 3))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");

    let err = receiver
        .funcall(
            &mrb,
            c"error",
            &[
                mrb.str_new(b"boom").as_value(),
                Value::from_int(&mrb, 1),
                Value::nil(),
            ],
        )
        .expect_err("an Integer where a nilable String is expected must raise");

    let Error::Exception(exc) = &err else {
        panic!("a rejected argument raises, got {err:?}");
    };
    assert_eq!(exc.classname(&mrb), "TypeError");
}

#[test]
fn a_value_parameter_receives_the_argument_itself() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniPassthrough");
    class
        .define_method(&mrb, c"pass", beni::method!(passthrough, 1))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");
    let argument = mrb.str_new(b"same").as_value();

    let got = receiver
        .funcall(&mrb, c"pass", &[argument])
        .expect("any value is accepted");

    assert!(got.obj_equal(&mrb, argument));
}

#[test]
fn a_nilable_optional_tells_omission_from_an_explicit_nil() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniNilableOptional");
    class
        .define_method(&mrb, c"given", beni::method!(nilable_optional, 0, 1))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");
    let call = |args: &[Value]| {
        i32::from_value(
            receiver
                .funcall(&mrb, c"given", args)
                .expect("the call must not raise"),
        )
    };

    assert_eq!(call(&[]), Some(0), "omitted binds None");
    assert_eq!(
        call(&[Value::nil()]),
        Some(1),
        "an explicit nil binds Some(None)"
    );
    assert_eq!(
        call(&[Value::from_int(&mrb, 5)]),
        Some(2),
        "an Integer binds Some(Some(_))"
    );
}

// Asserts every call in `calls` raises `ArgumentError` to the caller.
fn assert_each_raises_argument_error(
    mrb: &Mrb,
    receiver: Value,
    name: &core::ffi::CStr,
    calls: &[Vec<Value>],
) {
    for args in calls {
        let err = receiver
            .funcall(mrb, name, args)
            .expect_err("a call the declared arity does not accept must surface as Err");
        let Error::Exception(exc) = err else {
            panic!("a wrong argument count must raise, not panic, got {err}");
        };
        assert_eq!(exc.classname(mrb), "ArgumentError");
    }
}

#[test]
fn optional_arity_raises_argument_error_outside_its_range() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniOptArity");
    class
        .define_method(&mrb, c"add", beni::method!(opt_add, 1, 1))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");
    let one = Value::from_int(&mrb, 1);

    assert_each_raises_argument_error(&mrb, receiver, c"add", &[vec![], vec![one, one, one]]);
}

#[test]
fn block_accepting_arity_raises_argument_error_on_wrong_count() {
    let mrb = open_mrb();
    let class = fresh_class(&mrb, c"BeniBlockArity");
    class
        .define_method(&mrb, c"apply", beni::method!(apply_block, 1, &))
        .expect("registering the typed method must succeed");
    let receiver = class.obj_new(&mrb, &[]).expect("the receiver constructs");
    let one = Value::from_int(&mrb, 1);

    assert_each_raises_argument_error(&mrb, receiver, c"apply", &[vec![], vec![one, one]]);
}
