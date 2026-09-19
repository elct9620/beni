use crate::support::open_mrb;
use beni::prelude::*;
use beni::typed_data::Obj;
use beni::{
    DataType, Error, FromValue, IntoValue, Mrb, RClass, RTypedData, TryConvert, TypedData, Value,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Point {
    x: i32,
}

static POINT_TYPE: DataType<Point> = DataType::new(c"BeniPoint");

// SAFETY: `BeniPoint` is marked by `define_point` before any wrap.
unsafe impl TypedData for Point {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniPoint").expect("BeniPoint is defined")
    }

    fn data_type() -> &'static DataType<Self> {
        &POINT_TYPE
    }
}

struct Other;

static OTHER_TYPE: DataType<Other> = DataType::new(c"BeniOther");

// SAFETY: `BeniOther` is marked by `define_point` before any wrap.
unsafe impl TypedData for Other {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniOther").expect("BeniOther is defined")
    }

    fn data_type() -> &'static DataType<Self> {
        &OTHER_TYPE
    }
}

fn define_marked(mrb: &Mrb, name: &core::ffi::CStr) -> RClass {
    let class = mrb
        .define_class(name, mrb.object_class())
        .expect("defining the carrier class must succeed");
    class
        .set_instance_data_tt(mrb)
        .expect("marking an ordinary class must succeed");
    class
}

fn define_point(mrb: &Mrb) -> RClass {
    define_marked(mrb, c"BeniOther");
    let class = define_marked(mrb, c"BeniPoint");
    class
        .define_method(mrb, c"x", beni::method!(point_x, 0))
        .expect("registering x must succeed");
    class
        .define_method(mrb, c"moved", beni::method!(point_moved, 1))
        .expect("registering moved must succeed");
    class
        .define_method(mrb, c"same?", beni::method!(point_same, 1))
        .expect("registering same? must succeed");
    class
}

fn point_x(_mrb: &Mrb, rb_self: &Point) -> i32 {
    rb_self.x
}

// Returning a `TypedData` value wraps it as a new instance.
fn point_moved(_mrb: &Mrb, rb_self: &Point, by: i32) -> Point {
    Point { x: rb_self.x + by }
}

fn point_same(mrb: &Mrb, rb_self: Obj<Point>, other: Obj<Point>) -> bool {
    rb_self.as_value().obj_equal(mrb, other.as_value())
}

fn type_error_message(mrb: &Mrb, err: Error) -> String {
    match err {
        Error::Exception(exc) => {
            assert_eq!(exc.classname(mrb), "TypeError");
            Error::Exception(exc).message(mrb)
        }
        other => panic!("a conversion failure raises a TypeError, got {other}"),
    }
}

#[test]
fn a_wrapped_value_reads_back_through_each_handle() {
    let mrb = open_mrb();
    define_point(&mrb);

    let obj = mrb.obj_wrap(Point { x: 7 });
    assert_eq!(obj.x, 7, "Obj<T> dereferences to the payload");
    assert!(obj.as_value().is_data());

    let untyped = mrb.wrap(Point { x: 8 });
    let got: &Point = untyped.get(&mrb).expect("the matching data type reads");
    assert_eq!(got.x, 8);
    assert_eq!(untyped.as_value().classname(&mrb), "BeniPoint");
}

#[test]
fn a_method_takes_its_receiver_and_returns_a_payload_as_typed_data() {
    let mrb = open_mrb();
    define_point(&mrb);
    let point = mrb.obj_wrap(Point { x: 2 }).as_value();

    let x = point
        .funcall(&mrb, c"x", &[])
        .expect("x reads the receiver's payload");
    assert_eq!(i32::from_value(x), Some(2));

    let moved = point
        .funcall(&mrb, c"moved", &[3i32.into_value(&mrb)])
        .expect("moved wraps its result");
    assert_eq!(moved.classname(&mrb), "BeniPoint");
    let x = moved
        .funcall(&mrb, c"x", &[])
        .expect("the new instance carries its payload");
    assert_eq!(i32::from_value(x), Some(5));

    let same = point
        .funcall(&mrb, c"same?", &[point])
        .expect("Obj<T> converts receiver and argument");
    assert!(same.is_true());
}

#[test]
fn a_mismatch_raises_mrubys_data_type_error() {
    let mrb = open_mrb();
    define_point(&mrb);
    let other = mrb.wrap(Other);

    let err = other
        .get::<Point>(&mrb)
        .err()
        .expect("another data type does not read as Point");
    assert_eq!(
        type_error_message(&mrb, err),
        "wrong argument type BeniOther (expected BeniPoint)"
    );

    let err = <&Point>::try_convert(mrb.str_new(b"s").as_value(), &mrb)
        .err()
        .expect("a String is no data carrier");
    assert_eq!(
        type_error_message(&mrb, err),
        "wrong argument type String (expected C data)"
    );

    let err = RTypedData::try_convert(Value::nil(), &mrb)
        .err()
        .expect("nil is no data carrier");
    assert_eq!(
        type_error_message(&mrb, err),
        "wrong argument type nil (expected C data)"
    );

    let err = mrb
        .obj_wrap(Point { x: 1 })
        .as_value()
        .funcall(&mrb, c"same?", &[1i32.into_value(&mrb)])
        .expect_err("an Integer argument does not convert to Obj<Point>");
    assert_eq!(
        type_error_message(&mrb, err),
        "wrong argument type Integer (expected C data)"
    );
}

#[test]
fn a_duplicated_carrier_holds_no_payload() {
    let mrb = open_mrb();
    define_point(&mrb);
    let point = mrb.obj_wrap(Point { x: 4 }).as_value();

    let copy = point
        .funcall(&mrb, c"dup", &[])
        .expect("mruby's dup copies the object");
    assert!(copy.is_data());
    assert!(
        RTypedData::try_convert(copy, &mrb).is_ok(),
        "a bare carrier is still a data carrier"
    );

    let err = copy
        .funcall(&mrb, c"x", &[])
        .expect_err("a bare carrier converts to no Point");
    assert_eq!(
        type_error_message(&mrb, err),
        "uninitialized BeniPoint (expected BeniPoint)"
    );
}

static UNMARKED_DROPS: AtomicUsize = AtomicUsize::new(0);

struct Unmarked;

impl Drop for Unmarked {
    fn drop(&mut self) {
        UNMARKED_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

static UNMARKED_TYPE: DataType<Unmarked> = DataType::new(c"BeniUnmarked");

// SAFETY: deliberately broken — `BeniUnmarked` is never marked, which
// is the contract breach the wrap must turn into a panic.
unsafe impl TypedData for Unmarked {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniUnmarked")
            .expect("BeniUnmarked is defined")
    }

    fn data_type() -> &'static DataType<Self> {
        &UNMARKED_TYPE
    }
}

#[test]
fn wrapping_into_an_unmarked_class_panics_after_reclaiming_the_payload() {
    UNMARKED_DROPS.store(0, Ordering::SeqCst);
    let mrb = open_mrb();
    mrb.define_class(c"BeniUnmarked", mrb.object_class())
        .expect("defining the class must succeed");

    let panicked =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mrb.wrap(Unmarked))).is_err();

    assert!(panicked, "a broken TypedData contract panics");
    assert_eq!(
        UNMARKED_DROPS.load(Ordering::SeqCst),
        1,
        "the payload is dropped once"
    );
    assert_eq!(mrb.str_new(b"alive").as_value().to_string(&mrb), "alive");
}

enum Shape {
    Round,
    Square,
}

static SHAPE_TYPE: DataType<Shape> = DataType::new(c"BeniShape");

// SAFETY: `BeniShape` and its subclasses are marked by the test before
// any wrap; a subclass of a marked class is marked.
unsafe impl TypedData for Shape {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniShape").expect("BeniShape is defined")
    }

    fn data_type() -> &'static DataType<Self> {
        &SHAPE_TYPE
    }

    fn class_for(mrb: &Mrb, value: &Self) -> RClass {
        match value {
            Shape::Round => mrb.class_get(c"BeniRound").expect("BeniRound is defined"),
            Shape::Square => Self::class(mrb),
        }
    }
}

#[test]
fn a_value_wraps_as_the_class_its_type_names_for_it() {
    let mrb = open_mrb();
    let shape = define_marked(&mrb, c"BeniShape");
    mrb.define_class(c"BeniRound", shape)
        .expect("a subclass of a marked class is marked");

    assert_eq!(
        mrb.wrap(Shape::Round).as_value().classname(&mrb),
        "BeniRound"
    );
    assert_eq!(
        mrb.wrap(Shape::Square).as_value().classname(&mrb),
        "BeniShape"
    );
    assert_eq!(
        Shape::Square.into_value(&mrb).classname(&mrb),
        "BeniShape",
        "IntoValue wraps as wrap does"
    );

    let round = mrb.class_get(c"BeniRound").expect("BeniRound is defined");
    let square_as_round = mrb.obj_wrap_as(Shape::Square, round);
    assert_eq!(square_as_round.as_value().classname(&mrb), "BeniRound");
    assert!(matches!(*square_as_round, Shape::Square));
}
