use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Error, Inline, InlineStruct, InlineType, IntoValue, Mrb, RClass, TryConvert, Value};

#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Vector2D {
    x: f64,
    y: f64,
}

static VECTOR2D: InlineType<Vector2D> = InlineType::new(c"Vector2D");

unsafe impl InlineStruct for Vector2D {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniVector2D")
            .expect("the vector class is defined before it is named")
    }

    fn inline_type() -> &'static InlineType<Self> {
        &VECTOR2D
    }
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Cell(u32);

static CELL: InlineType<Cell> = InlineType::new(c"Cell");

unsafe impl InlineStruct for Cell {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniCell")
            .expect("the cell class is defined before it is named")
    }

    fn inline_type() -> &'static InlineType<Self> {
        &CELL
    }
}

fn define(mrb: &Mrb, name: &core::ffi::CStr) -> RClass {
    mrb.define_class(name, mrb.object_class())
        .expect("defining an ordinary class must succeed")
}

fn prepared(mrb: &Mrb) -> RClass {
    let class = define(mrb, c"BeniVector2D");
    Vector2D::mark_carriers(mrb).expect("marking an ordinary class must succeed");
    class
}

fn exception(mrb: &Mrb, err: Error) -> (String, String) {
    match err {
        Error::Exception(exc) => (exc.classname(mrb), Error::Exception(exc).message(mrb)),
        other => panic!("expected an exception, got {other:?}"),
    }
}

#[test]
fn a_wrapped_value_reads_back_as_a_copy() {
    let mrb = open_mrb();
    prepared(&mrb);

    let inline = Inline::new(&mrb, Vector2D { x: 1.5, y: -2.0 });

    assert_eq!(inline.get(), Vector2D { x: 1.5, y: -2.0 });
    let converted = Inline::<Vector2D>::try_convert(inline.as_value(), &mrb)
        .expect("an inline struct of the type converts back");
    assert_eq!(converted.get(), Vector2D { x: 1.5, y: -2.0 });
}

#[test]
fn the_payload_lives_inside_the_object_across_a_collection() {
    let mrb = open_mrb();
    prepared(&mrb);
    let inline = Inline::new(&mrb, Vector2D { x: 3.0, y: 4.0 });
    let _root = mrb
        .gc_root(inline.as_value())
        .expect("rooting must succeed");

    mrb.full_gc();

    assert_eq!(inline.get(), Vector2D { x: 3.0, y: 4.0 });
}

#[test]
fn set_replaces_the_whole_payload() {
    let mrb = open_mrb();
    prepared(&mrb);
    let inline = Inline::new(&mrb, Vector2D { x: 1.0, y: 2.0 });

    inline
        .set(&mrb, Vector2D { x: 5.0, y: 6.0 })
        .expect("an unfrozen inline struct takes a new payload");

    assert_eq!(inline.get(), Vector2D { x: 5.0, y: 6.0 });
}

#[test]
fn set_on_a_frozen_receiver_is_a_frozen_error_leaving_the_payload() {
    let mrb = open_mrb();
    prepared(&mrb);
    let inline = Inline::new(&mrb, Vector2D { x: 1.0, y: 2.0 });
    inline.as_value().freeze(&mrb);

    let err = inline
        .set(&mrb, Vector2D { x: 9.0, y: 9.0 })
        .expect_err("a frozen inline struct refuses a new payload");

    assert_eq!(exception(&mrb, err).0, "FrozenError");
    assert_eq!(inline.get(), Vector2D { x: 1.0, y: 2.0 });
}

#[test]
fn dup_and_clone_copy_the_payload() {
    let mrb = open_mrb();
    prepared(&mrb);
    let inline = Inline::new(&mrb, Vector2D { x: 7.0, y: 8.0 });

    for copy in [
        inline.as_value().dup(&mrb).expect("dup must succeed"),
        inline
            .as_value()
            .funcall(&mrb, c"clone", &[])
            .expect("clone must succeed"),
    ] {
        let copy = Inline::<Vector2D>::try_convert(copy, &mrb)
            .expect("a copy converts as the original does");
        assert_eq!(copy.get(), Vector2D { x: 7.0, y: 8.0 });
    }
}

#[test]
fn ruby_cannot_allocate_an_instance() {
    let mrb = open_mrb();
    prepared(&mrb);

    let err = mrb
        .load_string(b"BeniVector2D.new")
        .expect_err("the default allocator is undefined");

    assert_eq!(
        exception(&mrb, err),
        (
            "TypeError".to_owned(),
            "allocator undefined for BeniVector2D".to_owned()
        )
    );
}

#[test]
fn a_value_of_another_kind_is_a_type_error_naming_it_as_mruby_does() {
    let mrb = open_mrb();
    prepared(&mrb);
    define(&mrb, c"BeniCell");
    Cell::mark_carriers(&mrb).expect("marking an ordinary class must succeed");
    let cell = Inline::new(&mrb, Cell(1)).as_value();
    let plain = mrb
        .object_class()
        .new_instance(&mrb, &[])
        .expect("an Object constructs");

    for (value, named) in [
        (beni::value::qnil().as_value(), "nil"),
        (3i32.into_value(&mrb), "Integer"),
        (plain, "Object"),
        (cell, "BeniCell"),
    ] {
        let err = Inline::<Vector2D>::try_convert(value, &mrb)
            .err()
            .expect("no other value converts");
        assert_eq!(
            exception(&mrb, err),
            (
                "TypeError".to_owned(),
                format!("wrong argument type {named} (expected Vector2D)")
            )
        );
    }
}

#[test]
fn a_class_refuses_the_mark_unless_its_instances_are_plain_or_its_own() {
    let mrb = open_mrb();
    prepared(&mrb);
    let string_kind = mrb
        .define_class(
            c"BeniText",
            mrb.class_get(c"String").expect("String is a core class"),
        )
        .expect("subclassing String must succeed");

    for class in [string_kind, Vector2D::class(&mrb)] {
        let err = class
            .set_instance_inline_tt::<Cell>(&mrb)
            .expect_err("only a plain class or one of the same type accepts");
        assert_eq!(exception(&mrb, err).0, "TypeError");
    }
    Vector2D::class(&mrb)
        .set_instance_inline_tt::<Vector2D>(&mrb)
        .expect("a class already belonging to the type accepts again");
}

#[test]
fn a_subclass_defined_after_the_mark_belongs_to_the_type() {
    let mrb = open_mrb();
    let parent = prepared(&mrb);
    let child = mrb
        .define_class(c"BeniVector3D", parent)
        .expect("subclassing a marked class must succeed");
    define(&mrb, c"BeniCell");

    child
        .set_instance_inline_tt::<Vector2D>(&mrb)
        .expect("a subclass belongs to its superclass's type");
    let err = child
        .set_instance_inline_tt::<Cell>(&mrb)
        .expect_err("a subclass does not belong to another type");
    assert_eq!(exception(&mrb, err).0, "TypeError");
}

#[beni::wrap(class = "BeniPoint2D", inline)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Point2D {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
#[beni(class = "BeniSize2D", name = "Size2D")]
#[repr(C)]
struct Size2D {
    w: f32,
    h: f32,
}

fn point_create(_mrb: &Mrb, _class: Value, x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

fn point_plus(_mrb: &Mrb, rb_self: Point2D, other: Point2D) -> Point2D {
    Point2D {
        x: rb_self.x + other.x,
        y: rb_self.y + other.y,
    }
}

// Read back whole, so the reading converts under either float width.
fn point_x(_mrb: &Mrb, rb_self: Point2D) -> i32 {
    rb_self.x as i32
}

fn point_set_x(mrb: &Mrb, rb_self: Inline<Point2D>, x: f64) -> Result<Value, Error> {
    rb_self.set(mrb, Point2D { x, ..rb_self.get() })?;
    Ok(beni::value::qnil().as_value())
}

fn define_point(mrb: &Mrb) {
    let class = define(mrb, c"BeniPoint2D");
    Point2D::mark_carriers(mrb).expect("marking an ordinary class must succeed");
    class
        .define_singleton_method(mrb, c"create", beni::method!(point_create, 2))
        .expect("registering create must succeed");
    class
        .define_method(mrb, c"+", beni::method!(point_plus, 1))
        .expect("registering + must succeed");
    class
        .define_method(mrb, c"x", beni::method!(point_x, 0))
        .expect("registering x must succeed");
    class
        .define_method(mrb, c"x=", beni::method!(point_set_x, 1))
        .expect("registering x= must succeed");
}

#[test]
fn a_wrapped_inline_struct_crosses_ruby_by_value() {
    let mrb = open_mrb();
    define_point(&mrb);

    let sum = mrb
        .load_string(b"a = BeniPoint2D.create(1.5, 2.0); b = a + a; a.x = 10.0; [a.x, b.x]")
        .expect("the program runs");

    let got = Vec::<i32>::try_convert(sum, &mrb).expect("an array of integers");
    assert_eq!(got, [10, 3], "a copy keeps its own payload");
}

#[test]
fn a_returned_value_and_its_conversion_round_trip() {
    let mrb = open_mrb();
    define_point(&mrb);

    let value = Point2D { x: -1.0, y: 0.5 }.into_value(&mrb);

    assert_eq!(value.classname(&mrb), "BeniPoint2D");
    assert_eq!(
        Point2D::try_convert(value, &mrb).expect("its own value converts back"),
        Point2D { x: -1.0, y: 0.5 }
    );
}

#[test]
fn a_frozen_receiver_refuses_a_setter_from_ruby() {
    let mrb = open_mrb();
    define_point(&mrb);

    let err = mrb
        .load_string(b"p = BeniPoint2D.create(1.0, 1.0).freeze; p.x = 2.0")
        .expect_err("a frozen inline struct refuses a new payload");

    assert_eq!(exception(&mrb, err).0, "FrozenError");
}

#[test]
fn the_derive_names_the_type_by_its_name_attribute() {
    let mrb = open_mrb();
    define_point(&mrb);
    define(&mrb, c"BeniSize2D");
    Size2D::mark_carriers(&mrb).expect("marking an ordinary class must succeed");
    let size = Size2D { w: 2.0, h: 3.0 }.into_value(&mrb);

    let err = Point2D::try_convert(size, &mrb).expect_err("another type's value does not convert");
    assert_eq!(
        exception(&mrb, err).1,
        "wrong argument type BeniSize2D (expected BeniPoint2D)"
    );
    let err = Size2D::try_convert(beni::value::qnil().as_value(), &mrb)
        .expect_err("nil does not convert");
    assert_eq!(
        exception(&mrb, err).1,
        "wrong argument type nil (expected Size2D)"
    );
}

#[test]
#[should_panic(expected = "never marked as a carrier class")]
fn naming_an_unmarked_class_panics_naming_mark_carriers() {
    let mrb = open_mrb();
    define(&mrb, c"BeniPoint2D");

    let _ = Point2D { x: 0.0, y: 0.0 }.into_value(&mrb);
}

#[test]
fn a_path_bound_to_another_class_after_marking_reaches_no_wrap() {
    let mrb = open_mrb();
    define_point(&mrb);
    mrb.load_string(b"Object.send(:remove_const, :BeniPoint2D); class BeniPoint2D; end")
        .expect("rebinding the constant runs");

    let value = Point2D { x: 1.0, y: 2.0 }.into_value(&mrb);

    assert!(
        Point2D::try_convert(value, &mrb).is_ok(),
        "the wrap reaches the class the record holds"
    );
}

#[test]
fn marking_again_holds_the_class_the_path_now_names() {
    let mrb = open_mrb();
    define_point(&mrb);
    mrb.load_string(b"Object.send(:remove_const, :BeniPoint2D); class BeniPoint2D; end")
        .expect("rebinding the constant runs");
    let remarked = mrb
        .class_get(c"BeniPoint2D")
        .expect("the path names the new class");

    Point2D::mark_carriers(&mrb).expect("the new class accepts the mark");
    let value = Point2D { x: 1.0, y: 2.0 }.into_value(&mrb);

    assert!(value
        .class(&mrb)
        .as_value()
        .is_equal(&mrb, remarked.as_value()));
}

#[test]
fn the_type_class_is_held_per_interpreter_once_marked() {
    let first = open_mrb();
    let second = open_mrb();
    define_point(&first);
    define(&second, c"BeniPoint2D");

    assert!(first.inline_carrier::<Point2D>().is_some());
    assert!(second.inline_carrier::<Point2D>().is_none());
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Stray {
    x: f64,
    y: f64,
}

static STRAY: InlineType<Stray> = InlineType::new(c"Stray");

// SAFETY: deliberately broken — names the vector's class — to observe
// what a wrap into another type's class does.
unsafe impl InlineStruct for Stray {
    fn class(mrb: &Mrb) -> RClass {
        mrb.class_get(c"BeniVector2D")
            .expect("the vector class is defined before it is named")
    }

    fn inline_type() -> &'static InlineType<Self> {
        &STRAY
    }
}

#[test]
fn wrapping_into_another_types_class_converts_as_that_type() {
    let mrb = open_mrb();
    prepared(&mrb);

    let value = Inline::new(&mrb, Stray { x: 1.0, y: 2.0 }).as_value();

    assert_eq!(
        Inline::<Vector2D>::try_convert(value, &mrb)
            .expect("the class belongs to Vector2D")
            .get(),
        Vector2D { x: 1.0, y: 2.0 }
    );
}

#[test]
#[should_panic(expected = "cannot carry inline structs")]
fn wrapping_into_a_class_never_marked_for_the_type_panics() {
    let mrb = open_mrb();
    define(&mrb, c"BeniVector2D");

    let _ = Inline::new(&mrb, Vector2D { x: 0.0, y: 0.0 });
}

#[test]
fn an_inline_struct_of_any_type_converts_into_rinlinestruct() {
    let mrb = open_mrb();
    prepared(&mrb);
    let inline = Inline::new(&mrb, Vector2D { x: 0.0, y: 0.0 }).as_value();
    let plain = mrb
        .object_class()
        .new_instance(&mrb, &[])
        .expect("an Object constructs");

    assert!(beni::RInlineStruct::from_value(inline).is_some());
    assert!(beni::RInlineStruct::from_value(plain).is_none());
    assert!(beni::RTypedData::from_value(inline).is_none());
}
