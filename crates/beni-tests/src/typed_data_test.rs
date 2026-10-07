use crate::support::{open_mrb, Is};
use beni::prelude::*;
use beni::typed_data::Obj;
use beni::{
    DataType, Error, FromValue, IntoValue, Mrb, RClass, RTypedData, TryConvert, TypedData, Value,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[beni::wrap(class = "BeniPoint")]
struct Point {
    x: i32,
}

#[beni::wrap(class = "BeniOther")]
struct Other;

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
    Point::mark_carriers(mrb).expect("marking the point's carrier must succeed");
    Other::mark_carriers(mrb).expect("marking the other carrier must succeed");
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
    rb_self.as_value().is_equal(mrb, other.as_value())
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
    assert!(obj.as_value().is::<beni::RTypedData>());

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
    assert!(same.is::<beni::Qtrue>());
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

    let err = RTypedData::try_convert(beni::value::qnil().as_value(), &mrb)
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
    assert!(copy.is::<beni::RTypedData>());
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

#[derive(Clone)]
#[beni::wrap(class = "BeniCounter")]
struct Counter {
    n: std::cell::Cell<i32>,
}

fn counter_n(_mrb: &Mrb, rb_self: &Counter) -> i32 {
    rb_self.n.get()
}

fn counter_bump(_mrb: &Mrb, rb_self: &Counter) -> i32 {
    rb_self.n.set(rb_self.n.get() + 1);
    rb_self.n.get()
}

fn define_counter(mrb: &Mrb) {
    use beni::typed_data::Dup;
    let class = define_marked(mrb, c"BeniCounter");
    Counter::mark_carriers(mrb).expect("marking the counter's carrier must succeed");
    class
        .define_method(mrb, c"n", beni::method!(counter_n, 0))
        .expect("registering n must succeed");
    class
        .define_method(mrb, c"bump", beni::method!(counter_bump, 0))
        .expect("registering bump must succeed");
    class
        .define_method(mrb, c"dup", beni::method!(<Counter as Dup>::dup, 0))
        .expect("registering dup must succeed");
    class
        .define_method(mrb, c"clone", beni::method!(<Counter as Dup>::clone, -1))
        .expect("registering clone must succeed");
}

fn read_n(mrb: &Mrb, obj: Value) -> i32 {
    let n = obj.funcall(mrb, c"n", &[]).expect("n reads the payload");
    i32::from_value(n).expect("n is an Integer")
}

#[test]
fn dup_carries_an_independent_copy_of_the_payload() {
    let mrb = open_mrb();
    define_counter(&mrb);
    let original = mrb
        .obj_wrap(Counter {
            n: std::cell::Cell::new(1),
        })
        .as_value();

    let copy = original.funcall(&mrb, c"dup", &[]).expect("dup copies");
    assert_eq!(copy.classname(&mrb), "BeniCounter");
    assert!(!copy.is_equal(&mrb, original), "dup answers a new object");
    assert_eq!(read_n(&mrb, copy), 1);

    copy.funcall(&mrb, c"bump", &[]).expect("bump the copy");
    assert_eq!(read_n(&mrb, copy), 2);
    assert_eq!(
        read_n(&mrb, original),
        1,
        "the original keeps its own payload"
    );
}

#[test]
fn clone_keeps_singleton_and_frozen_state_with_a_copied_payload() {
    let mrb = open_mrb();
    define_counter(&mrb);
    let wrapped = mrb.obj_wrap(Counter {
        n: std::cell::Cell::new(5),
    });
    let original = wrapped.as_value();
    wrapped
        .singleton_class(&mrb)
        .expect("a carrier has a singleton class")
        .define_method(&mrb, c"only_mine", beni::method!(counter_n, 0))
        .expect("registering a singleton method must succeed");
    original.freeze(&mrb);

    let copy = original.funcall(&mrb, c"clone", &[]).expect("clone copies");
    assert!(!copy.is_equal(&mrb, original), "clone answers a new object");
    assert_eq!(read_n(&mrb, copy), 5);
    let frozen = copy
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? answers");
    assert!(frozen.is::<beni::Qtrue>(), "clone keeps the frozen state");
    let mine = copy
        .funcall(&mrb, c"only_mine", &[])
        .expect("clone keeps the singleton class");
    assert_eq!(i32::from_value(mine), Some(5));

    copy.funcall(&mrb, c"bump", &[]).expect("bump the copy");
    assert_eq!(
        read_n(&mrb, original),
        5,
        "the original keeps its own payload"
    );
}

#[test]
fn clone_takes_no_arguments() {
    let mrb = open_mrb();
    define_counter(&mrb);
    let original = mrb
        .obj_wrap(Counter {
            n: std::cell::Cell::new(0),
        })
        .as_value();

    mrb.define_global_const("C", original).unwrap();
    let raised = |source: &str| {
        mrb.load_string(source.as_bytes())
            .map(|value| value.inspect(&mrb))
            .map_err(|err| err.message(&mrb))
    };

    let expected = "wrong number of arguments (given 1, expected 0)";
    assert_eq!(raised("C.clone(true)").unwrap_err(), expected);
    assert_eq!(raised("C.clone(freeze: false)").unwrap_err(), expected);
    assert!(
        raised("C.clone(**{})").is_ok(),
        "an empty keyword splat passes no argument"
    );
}

#[test]
fn a_data_carrier_converts_into_rtypeddata_and_any_other_value_does_not() {
    let mrb = open_mrb();
    define_point(&mrb);
    let carrier = mrb.obj_wrap(Point { x: 1 }).as_value();

    assert!(RTypedData::from_value(carrier).is_some());
    assert!(RTypedData::from_value(mrb.str_new(b"s").as_value()).is_none());
    assert!(RTypedData::from_value(beni::value::qnil().as_value()).is_none());
}

#[test]
fn a_data_carrier_keeps_an_instance_variable_through_either_handle() {
    let mrb = open_mrb();
    define_point(&mrb);
    let name = mrb.intern_cstr(c"@tag").expect("the name interns");

    let typed = mrb.obj_wrap(Point { x: 1 });
    typed
        .ivar_set(&mrb, name, 3i32)
        .expect("assigning on an unfrozen carrier does not raise");
    let untyped = mrb.wrap(Point { x: 2 });
    untyped
        .ivar_set(&mrb, name, 4i32)
        .expect("assigning on an unfrozen carrier does not raise");

    assert_eq!(typed.ivar_get::<_, i32>(&mrb, name).ok(), Some(3));
    assert_eq!(untyped.ivar_get::<_, i32>(&mrb, name).ok(), Some(4));
}

#[derive(Debug, PartialEq)]
#[beni::wrap(class = "BeniTally")]
struct Tally {
    n: i32,
}

// A hand-marked class keeps its default allocator, so `new` and
// `allocate` make bare carriers its `initialize` completes.
#[derive(Debug, PartialEq)]
struct Seed {
    n: i32,
}

static SEED_TYPE: DataType<Seed> = DataType::new(c"BeniSeed");

// SAFETY: every test using `Seed` marks `BeniSeed` first.
unsafe impl TypedData for Seed {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniSeed").expect("BeniSeed is defined")
    }

    fn data_type() -> &'static DataType<Self> {
        &SEED_TYPE
    }
}

fn refused<T: core::fmt::Debug>(mrb: &Mrb, offered: T) -> Error {
    Error::new(
        mrb,
        mrb.exc_get(c"RuntimeError").expect("RuntimeError is defined"),
        &format!("already holds a payload, refused {offered:?}"),
    )
}

fn seed_n(_mrb: &Mrb, rb_self: &Seed) -> i32 {
    rb_self.n
}

fn seed_initialize(mrb: &Mrb, rb_self: RTypedData, n: i32) -> Result<Value, Error> {
    rb_self
        .init(mrb, Seed { n })
        .map(|()| beni::value::qnil().as_value())
        .map_err(|offered| refused(mrb, offered))
}

fn define_seed(mrb: &Mrb) {
    let class = define_marked(mrb, c"BeniSeed");
    class
        .define_method(mrb, c"n", beni::method!(seed_n, 0))
        .expect("registering n must succeed");
    class
        .define_method(mrb, c"initialize", beni::method!(seed_initialize, 1))
        .expect("registering initialize must succeed");
}

fn tally_n(_mrb: &Mrb, rb_self: &Tally) -> i32 {
    rb_self.n
}

fn tally_initialize_copy(mrb: &Mrb, rb_self: RTypedData, orig: &Tally) -> Result<Value, Error> {
    rb_self
        .init(mrb, Tally { n: orig.n })
        .map(|()| beni::value::qnil().as_value())
        .map_err(|offered| refused(mrb, offered))
}

fn define_tally(mrb: &Mrb) -> RClass {
    let class = define_marked(mrb, c"BeniTally");
    Tally::mark_carriers(mrb).expect("marking the tally's carrier must succeed");
    class
        .define_method(mrb, c"n", beni::method!(tally_n, 0))
        .expect("registering n must succeed");
    class
        .define_method(mrb, c"initialize_copy", beni::method!(tally_initialize_copy, 1))
        .expect("registering initialize_copy must succeed");
    class
}

#[test]
fn initialize_installs_the_payload_into_the_carrier_new_makes() {
    let mrb = open_mrb();
    define_seed(&mrb);

    let n = mrb
        .load_string(b"BeniSeed.new(5).n")
        .expect("initialize completes the carrier new made");

    assert_eq!(i32::from_value(n), Some(5));
}

#[test]
fn mrubys_own_copies_keep_subclass_and_frozen_state_through_initialize_copy() {
    let mrb = open_mrb();
    let tally = define_tally(&mrb);
    let sub = mrb
        .define_class(c"BeniSubTally", tally)
        .expect("defining the subclass must succeed");
    let original = mrb.wrap_as(Tally { n: 3 }, sub).as_value();
    mrb.define_global_const(c"BENI_TALLY", original)
        .expect("naming the original must succeed");

    let got = mrb
        .load_string(
            b"t = BENI_TALLY.freeze
              d = t.dup
              c = t.clone
              [d.class == BeniSubTally, d.n, c.frozen?, c.n] == [true, 3, true, 3]",
        )
        .expect("initialize_copy completes each copy");

    assert_eq!(bool::from_value(got), Some(true));
}

#[test]
fn a_frozen_carrier_holding_no_payload_still_installs() {
    let mrb = open_mrb();
    define_seed(&mrb);
    let carrier = mrb
        .load_string(b"BeniSeed.allocate.freeze")
        .expect("allocate makes a bare carrier");
    let carrier = RTypedData::try_convert(carrier, &mrb).expect("a bare carrier is a data carrier");

    carrier
        .init(&mrb, Seed { n: 7 })
        .expect("a frozen bare carrier accepts its payload");

    assert_eq!(carrier.get::<Seed>(&mrb).map(|seed| seed.n).ok(), Some(7));
}

#[test]
fn a_carrier_holding_a_payload_refuses_and_hands_the_offer_back() {
    let mrb = open_mrb();
    define_seed(&mrb);
    let carrier = mrb
        .load_string(b"BeniSeed.new(1)")
        .expect("new completes the carrier");
    let carrier = RTypedData::try_convert(carrier, &mrb).expect("the seed is a data carrier");

    let answer = carrier.init(&mrb, Seed { n: 9 });

    assert_eq!(answer, Err(Seed { n: 9 }));
    assert_eq!(carrier.get::<Seed>(&mrb).map(|seed| seed.n).ok(), Some(1));
}

#[derive(Clone, Debug, PartialEq)]
#[beni::wrap(class = "BeniLedger")]
struct Ledger {
    n: i32,
}

fn ledger_n(_mrb: &Mrb, rb_self: &Ledger) -> i32 {
    rb_self.n
}

fn ledger_initialize_copy(mrb: &Mrb, rb_self: RTypedData, orig: &Ledger) -> Result<Value, Error> {
    rb_self
        .init(mrb, Ledger { n: orig.n + 100 })
        .map(|()| beni::value::qnil().as_value())
        .map_err(|offered| refused(mrb, offered))
}

#[test]
fn dup_clone_keeps_the_payload_initialize_copy_installed() {
    use beni::typed_data::Dup;
    let mrb = open_mrb();
    let class = define_marked(&mrb, c"BeniLedger");
    Ledger::mark_carriers(&mrb).expect("marking the ledger's carrier must succeed");
    class
        .define_method(&mrb, c"n", beni::method!(ledger_n, 0))
        .expect("registering n must succeed");
    class
        .define_method(&mrb, c"initialize_copy", beni::method!(ledger_initialize_copy, 1))
        .expect("registering initialize_copy must succeed");
    class
        .define_method(&mrb, c"clone", beni::method!(<Ledger as Dup>::clone, -1))
        .expect("registering clone must succeed");
    let original = mrb.wrap(Ledger { n: 3 });

    let copy = original
        .as_value()
        .funcall(&mrb, c"clone", &[])
        .expect("clone completes the copy");

    assert_eq!(read_ledger(&mrb, copy), 103);
}

fn read_ledger(mrb: &Mrb, obj: Value) -> i32 {
    let n = obj.funcall(mrb, c"n", &[]).expect("n reads the payload");
    i32::from_value(n).expect("n is an Integer")
}
