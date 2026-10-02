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
        inline.as_value().obj_dup(&mrb).expect("dup must succeed"),
        inline
            .as_value()
            .obj_clone(&mrb)
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
        .obj_new(&mrb, &[])
        .expect("an Object constructs");

    for (value, named) in [
        (Value::nil(), "nil"),
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
