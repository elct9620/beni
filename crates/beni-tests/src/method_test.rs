use beni::{Error, FromValue, IntoValue, Mrb, Value};
use std::sync::atomic::{AtomicBool, Ordering};

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
        .class_get(c"RuntimeError")
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = fresh_class(&mrb, c"BeniArityAdder");
    class
        .define_method(&mrb, c"add", beni::method!(add, 2))
        .expect("registering the fixed-arity method must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // `mrb_get_args` enforces the `MRB_ARGS_REQ(2)` count before any
    // `FromValue` conversion runs, raising `ArgumentError` whose
    // longjmp crosses the bridge's `catch_unwind` — both too few and
    // too many positionals take that path.
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
            Error::Panic(_) => panic!("a wrong argument count must raise, not panic"),
        }
    }

    // The count-error longjmp left the VM intact: a correctly-counted
    // call still dispatches and returns.
    let got = receiver
        .funcall(
            &mrb,
            c"add",
            &[Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)],
        )
        .expect("the VM survives the count-error longjmp and the next call runs");
    assert_eq!(i32::from_value(got), Some(3));
}

#[test]
fn from_value_failure_raises_before_body_runs() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = fresh_class(&mrb, c"BeniBlockApply");
    class
        .define_method(&mrb, c"apply", beni::method!(apply_block, 1, &))
        .expect("registering the block-accepting method must succeed");

    // Calling with a block: the registered method receives it as
    // `Some(Proc)` and yields the argument to it, here doubling it.
    let cxt = beni::Ccontext::new(&mrb, c"block_method_test.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt.load_nstring(b"BeniBlockApply.new.apply(21) { |x| x * 2 }");
    assert!(
        mrb.pending_exc().is_nil(),
        "the block-yielding call must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn block_accepting_method_binds_none_without_a_block() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let err = mrb
        .protect(|_| panic!("pop goes the closure"))
        .expect_err("the panic must surface as Err");
    match err {
        Error::Panic(msg) => assert!(msg.contains("pop goes the closure")),
        Error::Exception(_) => panic!("a closure panic must surface as Error::Panic"),
    }
}

#[test]
fn result_returning_method_raises_its_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
