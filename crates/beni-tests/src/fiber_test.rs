use beni::prelude::*;
use beni::{Error, Fiber, FromValue, IntoValue, Mrb, Proc, Value};

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
    unsafe extern "C" fn stub(
        _mrb: *mut beni::sys::mrb_state,
        self_: beni::sys::mrb_value,
    ) -> beni::sys::mrb_value {
        self_
    }

    let mrb = open_mrb();
    // SAFETY: `stub` has the `mrb_func_t` ABI and is never called here;
    // `mrb_obj_value` boxes the RProc the constructor just returned.
    let value = unsafe {
        let raw = beni::sys::mrb_proc_new_cfunc(mrb.as_ptr(), stub);
        <Value as beni::sys::FromRawValue>::from_raw(beni::sys::mrb_obj_value(raw.cast()))
    };
    let cfunc = Proc::from_value(value).expect("the constructor answers a Proc-tagged value");

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
