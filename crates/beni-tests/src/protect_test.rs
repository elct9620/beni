use crate::support::open_mrb;
use beni::prelude::*;
use beni::Error;

#[test]
fn protect_returns_the_body_value_on_success() {
    let mrb = open_mrb();

    let got = beni::sys::protect(&mrb, |m| m.str_new(b"ok").as_value())
        .expect("a non-raising body must come back Ok");

    assert_eq!(got.to_string(&mrb), "ok");
}

#[test]
fn protect_surfaces_a_raw_binding_raise_as_err_on_a_clean_handle() {
    let mrb = open_mrb();

    let err = beni::sys::protect(&mrb, |m| {
        // SAFETY: `m` is the live VM inside the protected frame;
        // `RuntimeError` is a core class so the lookup cannot fail;
        // `mrb_raise` long-jumps to the protect frame and never returns
        // here, and this frame holds nothing to drop.
        unsafe {
            let runtime_error = beni::sys::mrb_class_get(m.as_ptr(), c"RuntimeError".as_ptr());
            beni::sys::mrb_raise(m.as_ptr(), runtime_error, c"boom from ruby".as_ptr());
        }
    })
    .expect_err("a raise inside the body must surface as Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("boom from ruby")),
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "the caught exception must not stay pending on the handle"
    );
}

#[test]
fn catch_unwind_answers_the_closure_value() {
    let got = beni::sys::catch_unwind(|| 7).expect("a closure that returns must come back Ok");

    assert_eq!(got, 7);
}

#[test]
fn catch_unwind_surfaces_a_panic_as_err_carrying_its_message() {
    let err = beni::sys::catch_unwind(|| -> i32 { panic!("boom from a callback") })
        .expect_err("a panic inside the closure must surface as Err");

    match err {
        Error::Panic(msg) => assert_eq!(msg, "boom from a callback"),
        other => panic!("a panic must surface as Error::Panic, got {other}"),
    }
}
