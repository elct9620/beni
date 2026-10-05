use beni::prelude::*;
use beni::{Error, Fiber, FiberYield, FromValue, IntoValue, Mrb, Proc, Value};

use crate::support::{open_mrb, same_object};

fn fiber(mrb: &Mrb, source: &str) -> Fiber {
    let value = mrb
        .load_string(source.as_bytes())
        .expect("the fiber source runs");
    Fiber::from_value(value).expect("the source answers a Fiber")
}

fn inspect(mrb: &Mrb, value: Value) -> String {
    let rendered = value.funcall(mrb, "inspect", &[]).expect("inspect renders");
    String::from_utf8(Vec::<u8>::from_value(rendered).expect("inspect answers a String"))
        .expect("inspect renders UTF-8")
}

fn assert_fiber_error(mrb: &Mrb, err: &Error, wording: &str) {
    let fiber_error = mrb
        .exc_get("FiberError")
        .expect("the fiber gem defines FiberError");
    assert!(
        err.is_kind_of(mrb, fiber_error),
        "expected FiberError, got {}",
        err.message(mrb)
    );
    assert!(
        err.message(mrb).contains(wording),
        "expected {wording:?}, got {:?}",
        err.message(mrb)
    );
}

#[test]
fn a_fiber_from_a_ruby_block_resumes_until_its_block_finishes() {
    let mrb = open_mrb();
    let block = mrb
        .load_string(b"proc { |a| b = Fiber.yield(a + 1); b * 2 }")
        .unwrap();
    let fiber = mrb
        .fiber_new(Proc::from_value(block).unwrap())
        .expect("a Ruby-defined block makes a fiber");

    assert!(
        fiber.is_alive(&mrb).unwrap(),
        "a created fiber has not finished"
    );
    assert_eq!(fiber.resume::<i32>(&mrb, &[5.into_value(&mrb)]).unwrap(), 6);
    assert_eq!(
        fiber.resume::<i32>(&mrb, &[7.into_value(&mrb)]).unwrap(),
        14
    );
    assert!(!fiber.is_alive(&mrb).unwrap(), "the block has finished");

    let err = fiber.resume::<Value>(&mrb, &[]).unwrap_err();
    assert_fiber_error(&mrb, &err, "dead fiber");
}

#[test]
fn a_slice_crossing_a_switch_arrives_as_one_value() {
    let mrb = open_mrb();
    let fiber = fiber(&mrb, "Fiber.new { |*a| r = [a]; r << Fiber.yield; r << Fiber.yield(1); r << Fiber.yield(1, 2); r }");
    let one = 1.into_value(&mrb);
    let two = 2.into_value(&mrb);

    let yields = [
        fiber.resume::<Value>(&mrb, &[]).unwrap(),
        fiber.resume::<Value>(&mrb, &[]).unwrap(),
        fiber.resume::<Value>(&mrb, &[one]).unwrap(),
    ];
    let finished = fiber.resume::<Value>(&mrb, &[one, two]).unwrap();

    let rendered: Vec<String> = yields.iter().map(|v| inspect(&mrb, *v)).collect();
    assert_eq!(rendered, ["nil", "1", "[1, 2]"]);
    assert_eq!(inspect(&mrb, finished), "[[], nil, 1, [1, 2]]");
}

#[test]
fn a_raise_inside_the_fiber_surfaces_and_leaves_the_resumer_current() {
    let mrb = open_mrb();
    let root = mrb.load_string(b"Fiber.current").unwrap();
    let fiber = fiber(
        &mrb,
        "Fiber.new { raise ArgumentError, 'inside the fiber' }",
    );

    let err = fiber.resume::<Value>(&mrb, &[]).unwrap_err();

    let argument_error = mrb.exc_get("ArgumentError").unwrap();
    assert!(err.is_kind_of(&mrb, argument_error));
    assert!(err.message(&mrb).contains("inside the fiber"));
    let current = mrb.load_string(b"Fiber.current").unwrap();
    assert!(
        same_object(&mrb, root, current),
        "the resumer's fiber is current again"
    );
    let next = fiber_after_raise(&mrb);
    assert_eq!(next, 3, "the interpreter keeps switching fibers");
}

fn fiber_after_raise(mrb: &Mrb) -> i32 {
    fiber(mrb, "Fiber.new { Fiber.yield 3 }")
        .resume(mrb, &[])
        .unwrap()
}

#[test]
fn a_fiber_resuming_itself_is_refused() {
    fn resume_self(mrb: &Mrb, _self: Value, fiber: Fiber) -> Result<Value, Error> {
        fiber.resume(mrb, &[])
    }

    let mrb = open_mrb();
    mrb.object_class()
        .define_singleton_method(&mrb, "resume_self", beni::method!(resume_self, 1))
        .unwrap();
    let fiber = fiber(
        &mrb,
        "f = Fiber.new { begin; Object.resume_self(f); rescue FiberError => e; e.message; end }; f",
    );

    let message = fiber.resume::<Value>(&mrb, &[]).unwrap();

    assert!(
        inspect(&mrb, message).contains("current fiber"),
        "the running fiber cannot be resumed"
    );
}

#[test]
fn an_uninitialized_fiber_answers_fiber_error() {
    let mrb = open_mrb();
    let fiber = fiber(&mrb, "Fiber.allocate");

    assert_fiber_error(&mrb, &fiber.is_alive(&mrb).unwrap_err(), "uninitialized");
    assert_fiber_error(
        &mrb,
        &fiber.resume::<Value>(&mrb, &[]).unwrap_err(),
        "uninitialized",
    );
}

#[test]
fn a_block_backed_by_a_c_function_makes_no_fiber() {
    fn body(_mrb: &Mrb, _args: &[Value], _block: Option<Proc>) -> bool {
        true
    }

    let mrb = open_mrb();
    let cfunc = mrb.proc_new(body);

    let err = match mrb.fiber_new(cfunc) {
        Err(err) => err,
        Ok(_) => panic!("a C function cannot be a fiber's body"),
    };

    assert_fiber_error(&mrb, &err, "C defined method");
}

#[test]
fn a_resumed_value_converts_to_the_requested_type() {
    let mrb = open_mrb();
    let fiber = fiber(&mrb, "Fiber.new { 'not a number' }");

    let err = fiber.resume::<i32>(&mrb, &[]).unwrap_err();

    let type_error = mrb.exc_get("TypeError").unwrap();
    assert!(
        err.is_kind_of(&mrb, type_error),
        "the conversion's error surfaces"
    );
}

fn current(mrb: &Mrb, _self: Value) -> Result<Fiber, Error> {
    mrb.fiber_current()
}

#[test]
fn the_current_fiber_is_the_root_outside_any_resumed_fiber() {
    let mrb = open_mrb();
    let root = mrb.load_string(b"Fiber.current").unwrap();

    let current = mrb.fiber_current().unwrap();

    assert!(same_object(&mrb, root, current));
}

#[test]
fn the_current_fiber_inside_a_resumed_fiber_is_that_fiber() {
    let mrb = open_mrb();
    mrb.object_class()
        .define_singleton_method(&mrb, "rust_current", beni::method!(current, 0))
        .unwrap();

    let found = mrb
        .load_string(b"f = Fiber.new { Object.rust_current }; f.resume.equal?(f)")
        .unwrap();

    assert!(found.to_bool(), "the read answers the resumed fiber");
}

#[test]
fn a_redefined_current_fiber_read_surfaces_its_failure() {
    let mrb = open_mrb();
    mrb.load_string(b"def Fiber.current; :not_a_fiber; end")
        .unwrap();

    let Err(err) = mrb.fiber_current() else {
        panic!("the read answered a Fiber");
    };

    let type_error = mrb.exc_get("TypeError").unwrap();
    assert!(
        err.is_kind_of(&mrb, type_error),
        "got {}",
        err.message(&mrb)
    );

    mrb.load_string(b"def Fiber.current; raise ArgumentError, 'no fiber'; end")
        .unwrap();

    let Err(err) = mrb.fiber_current() else {
        panic!("the read answered a Fiber");
    };

    assert!(err.message(&mrb).contains("no fiber"));
}

#[test]
fn a_current_fiber_read_without_a_fiber_class_surfaces_name_error() {
    let mrb = open_mrb();
    mrb.load_string(b"Object.send(:remove_const, :Fiber)")
        .unwrap();

    let Err(err) = mrb.fiber_current() else {
        panic!("the read answered a Fiber");
    };

    let name_error = mrb.exc_get("NameError").unwrap();
    assert!(
        err.is_kind_of(&mrb, name_error),
        "got {}",
        err.message(&mrb)
    );
}

fn pause(mrb: &Mrb, _self: Value, args: &[Value]) -> FiberYield {
    mrb.fiber_yield(args)
}

fn refuse(mrb: &Mrb, _self: Value) -> Result<FiberYield, Error> {
    Err(Error::new(
        mrb,
        mrb.exc_get("RuntimeError").unwrap(),
        "refused before yielding",
    ))
}

fn pause_through_dispatch(mrb: &Mrb, _self: Value, arg: Value) -> Result<Value, Error> {
    mrb.object_class().as_value().funcall(mrb, "pause", &[arg])
}

fn with_pause(mrb: &Mrb) {
    let object = mrb.object_class();
    object
        .define_singleton_method(mrb, "pause", beni::method!(pause, -1))
        .unwrap();
    object
        .define_singleton_method(mrb, "refuse", beni::method!(refuse, 0))
        .unwrap();
    object
        .define_singleton_method(
            mrb,
            "pause_through_dispatch",
            beni::method!(pause_through_dispatch, 1),
        )
        .unwrap();
}

#[test]
fn a_method_returning_a_fiber_yield_suspends_its_fiber() {
    let mrb = open_mrb();
    with_pause(&mrb);
    let fiber = fiber(
        &mrb,
        "Fiber.new { a = Object.pause(1); b = Object.pause(a + 1, 3); [a, b] }",
    );
    let ten = 10.into_value(&mrb);

    let first = fiber.resume::<Value>(&mrb, &[]).unwrap();
    let second = fiber.resume::<Value>(&mrb, &[ten]).unwrap();
    let finished = fiber.resume::<Value>(&mrb, &[ten, ten]).unwrap();

    assert_eq!(inspect(&mrb, first), "1");
    assert_eq!(inspect(&mrb, second), "[11, 3]");
    assert_eq!(
        inspect(&mrb, finished),
        "[10, [10, 10]]",
        "each resume's slice is the method's return"
    );
    assert!(!fiber.is_alive(&mrb).unwrap());
}

#[test]
fn a_fiber_yield_suspends_a_fiber_ruby_resumes() {
    let mrb = open_mrb();
    with_pause(&mrb);

    let results = mrb
        .load_string(
            b"f = Fiber.new { a = Object.pause(1); b = Object.pause(a + 1, 3); [a, b] }
              [f.resume, f.resume(10), f.resume(10, 10), f.alive?]",
        )
        .unwrap();

    assert_eq!(
        inspect(&mrb, results),
        "[1, [11, 3], [10, [10, 10]], false]",
        "each Ruby resume's arguments are the method's return"
    );
}

#[test]
fn a_fiber_yield_outside_a_resumed_fiber_raises_fiber_error() {
    let mrb = open_mrb();
    with_pause(&mrb);

    let message = mrb
        .load_string(b"begin; Object.pause(1); rescue FiberError => e; e.message; end")
        .unwrap();

    assert!(
        inspect(&mrb, message).contains("not resumed"),
        "got {}",
        inspect(&mrb, message)
    );
}

#[test]
fn a_fiber_yield_behind_a_dispatch_from_rust_raises_fiber_error() {
    let mrb = open_mrb();
    with_pause(&mrb);
    let fiber = fiber(
        &mrb,
        "Fiber.new { begin; Object.pause_through_dispatch(1); rescue FiberError => e; e.message; end }",
    );

    let message = fiber.resume::<Value>(&mrb, &[]).unwrap();

    assert!(
        inspect(&mrb, message).contains("C function boundary"),
        "got {}",
        inspect(&mrb, message)
    );
    assert!(
        !fiber.is_alive(&mrb).unwrap(),
        "the fiber ran to its end without switching"
    );
}

#[test]
fn an_err_before_the_fiber_yield_raises_without_suspending() {
    let mrb = open_mrb();
    with_pause(&mrb);
    let fiber = fiber(
        &mrb,
        "Fiber.new { begin; Object.refuse; rescue => e; e.message; end }",
    );

    let message = fiber.resume::<Value>(&mrb, &[]).unwrap();

    assert_eq!(inspect(&mrb, message), "\"refused before yielding\"");
}
