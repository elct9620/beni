use crate::support::open_mrb;
use beni::{Error, Mrb, Value};

#[test]
fn protect_returns_the_body_value_on_success() {
    let mrb = open_mrb();

    let got = mrb
        .protect(|m| m.str_new(b"ok").as_value())
        .expect("a non-raising body must come back Ok");

    assert_eq!(got.to_string(&mrb), "ok");
}

#[test]
fn protect_surfaces_a_raised_ruby_exception_as_err() {
    let mrb = open_mrb();

    let err = mrb
        .protect(|m| {
            // SAFETY: `m` is the live VM inside the protected frame;
            // `RuntimeError` is a core class so the lookup cannot
            // fail; `mrb_raise` long-jumps to the protect frame and
            // never returns here.
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
    // The VM stays usable after the protected raise.
    let again = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive the protected raise");
    assert_eq!(again.to_string(&mrb), "alive");
}

#[test]
fn protect_surfaces_a_panicking_body_as_err() {
    let mrb = open_mrb();

    let err = mrb
        .protect(|_| panic!("boom from rust"))
        .expect_err("a panic inside the body must surface as Err");

    match err {
        Error::Panic(msg) => assert!(msg.contains("boom from rust")),
        other => panic!("a Rust panic must surface as Error::Panic, got {other}"),
    }
    // The VM stays usable after the caught panic.
    let again = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive the caught panic");
    assert_eq!(again.to_string(&mrb), "alive");
}

/// Raise an instance of the named core exception class inside a
/// protected body, long-jumping to the protect frame the way a real
/// Ruby raise does (mirrors the `protect` raise test).
fn raise_named(m: &Mrb, class: &core::ffi::CStr, message: &core::ffi::CStr) -> Value {
    // SAFETY: `m` is the live VM; the named classes are core so the
    // lookup cannot fail; `mrb_raise` long-jumps and never returns.
    unsafe {
        let class = beni::sys::mrb_class_get(m.as_ptr(), class.as_ptr());
        beni::sys::mrb_raise(m.as_ptr(), class, message.as_ptr());
    }
}

#[test]
fn rescue_returns_the_body_value_when_it_does_not_raise() {
    let mrb = open_mrb();
    let standard_error = mrb
        .class_get(c"StandardError")
        .expect("StandardError is a core class");

    let got = mrb
        .rescue(
            &[standard_error],
            |m| m.str_new(b"body ok").as_value(),
            |_, _| panic!("the handler must not run when the body succeeds"),
        )
        .expect("a non-raising body comes back Ok");

    assert_eq!(got.to_string(&mrb), "body ok");
}

#[test]
fn rescue_runs_the_handler_on_a_clean_vm_for_a_matching_exception() {
    let mrb = open_mrb();
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is a core class");

    // RuntimeError is a StandardError, so a StandardError filter matches it.
    let standard_error = mrb
        .class_get(c"StandardError")
        .expect("StandardError is a core class");

    let got = mrb
        .rescue(
            &[standard_error],
            |m| raise_named(m, c"RuntimeError", c"boom"),
            |m, exc| {
                // The handler receives the caught exception itself.
                assert!(exc.is_kind_of(m, runtime_error));
                assert!(exc.to_string(m).contains("boom"));
                // The handler runs on a clean VM: no pending exception
                // remains on the handle, and a fresh allocation works.
                assert!(m.pending_exc().is_nil());
                m.str_new(b"handled").as_value()
            },
        )
        .expect("a matching exception is rescued by the handler");

    assert_eq!(got.to_string(&mrb), "handled");
    // The VM stays usable afterwards.
    let again = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive a rescued exception");
    assert_eq!(again.to_string(&mrb), "alive");
}

#[test]
fn rescue_propagates_an_exception_outside_the_class_list() {
    let mrb = open_mrb();
    // TypeError is not a kind of ArgumentError, so the filter misses.
    let argument_error = mrb
        .class_get(c"ArgumentError")
        .expect("ArgumentError is a core class");

    let err = mrb
        .rescue(
            &[argument_error],
            |m| raise_named(m, c"TypeError", c"wrong type"),
            |_, _| panic!("the handler must not run for an unmatched exception"),
        )
        .expect_err("an unmatched exception propagates as the body's Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("wrong type")),
        other => panic!("an unmatched Ruby raise stays Error::Exception, got {other}"),
    }
}

#[test]
fn rescue_surfaces_a_handler_raise_as_err() {
    let mrb = open_mrb();
    let standard_error = mrb
        .class_get(c"StandardError")
        .expect("StandardError is a core class");

    let err = mrb
        .rescue(
            &[standard_error],
            |m| raise_named(m, c"RuntimeError", c"boom"),
            |m, _| raise_named(m, c"RuntimeError", c"handler boom"),
        )
        .expect_err("a handler that raises surfaces as Err");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("handler boom")),
        other => panic!("a handler Ruby raise stays Error::Exception, got {other}"),
    }
}

#[test]
fn rescue_surfaces_a_handler_panic_as_err() {
    let mrb = open_mrb();
    let standard_error = mrb
        .class_get(c"StandardError")
        .expect("StandardError is a core class");

    // A matching exception routes into the handler, which then panics.
    // The handler runs under exception protection, so the panic is caught
    // at the FFI boundary as Error::Panic rather than being rescued.
    let err = mrb
        .rescue(
            &[standard_error],
            |m| raise_named(m, c"RuntimeError", c"boom"),
            |_, _| panic!("boom from handler"),
        )
        .expect_err("a handler panic surfaces as Err, not rescued");

    match err {
        Error::Panic(msg) => assert!(msg.contains("boom from handler")),
        other => panic!("a handler Rust panic must surface as Error::Panic, got {other}"),
    }
    // The VM stays usable after the caught handler panic.
    let again = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive the caught handler panic");
    assert_eq!(again.to_string(&mrb), "alive");
}

#[test]
fn rescue_with_an_empty_class_list_rescues_nothing() {
    let mrb = open_mrb();

    let err = mrb
        .rescue(
            &[],
            |m| raise_named(m, c"RuntimeError", c"boom"),
            |_, _| panic!("an empty class list never rescues"),
        )
        .expect_err("an empty class list lets every exception propagate");

    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("boom")),
        other => panic!("the raise stays Error::Exception, got {other}"),
    }
}

#[test]
fn rescue_does_not_catch_a_body_panic() {
    let mrb = open_mrb();
    let standard_error = mrb
        .class_get(c"StandardError")
        .expect("StandardError is a core class");

    let err = mrb
        .rescue(
            &[standard_error],
            |_| panic!("boom from rust"),
            |_, _| panic!("a panic is not a Ruby exception and is never rescued"),
        )
        .expect_err("a body panic surfaces as Err, not rescued");

    match err {
        Error::Panic(msg) => assert!(msg.contains("boom from rust")),
        other => panic!("a Rust panic must surface as Error::Panic, got {other}"),
    }
    // The VM stays usable after the caught panic.
    let again = mrb
        .protect(|m| m.str_new(b"alive").as_value())
        .expect("the VM must survive the caught panic");
    assert_eq!(again.to_string(&mrb), "alive");
}
