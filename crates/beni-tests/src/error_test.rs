use beni::{Error, Mrb};

#[test]
fn new_builds_an_exception_error_carrying_the_message() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is a core class");

    let err = Error::new(&mrb, runtime_error, "boom");

    // The constructor produces the Exception variant, and the
    // exception renders the message it was built with.
    assert!(matches!(err, Error::Exception(_)));
    assert_eq!(err.message(&mrb), "boom");
}

#[test]
fn argnum_renders_the_fixed_count_form() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // min == max: "wrong number of arguments (given 3, expected 2)".
    let err = Error::argnum(&mrb, 3, 2, 2);

    assert!(matches!(err, Error::Exception(_)));
    let exc = match err {
        Error::Exception(v) => v,
        Error::Panic(_) => unreachable!("argnum must surface as Error::Exception"),
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
    let message = Error::Exception(exc).message(&mrb);
    assert!(
        message.contains("given 3") && message.contains("expected 2"),
        "unexpected message: {message}"
    );
}

#[test]
fn argnum_renders_the_open_ended_form_for_a_negative_max() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // max < 0: "expected 2+" — at least `min`, no upper bound.
    let err = Error::argnum(&mrb, 1, 2, -1);

    let message = err.message(&mrb);
    assert!(
        message.contains("given 1") && message.contains("expected 2+"),
        "unexpected message: {message}"
    );
}

#[test]
fn argnum_renders_the_range_form_for_distinct_bounds() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // min < max: "expected 2..4" — an inclusive range.
    let err = Error::argnum(&mrb, 5, 2, 4);

    let message = err.message(&mrb);
    assert!(
        message.contains("given 5") && message.contains("expected 2..4"),
        "unexpected message: {message}"
    );
}
