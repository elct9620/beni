//! The `Error` type surfaced across the mruby boundary.
//!
//! Mirrors the role of `magnus::Error`: every safe operation that
//! mruby can reject — class/module definition, method registration,
//! constant lookup, protected execution — returns `Result<_, Error>`
//! instead of letting the raise long-jump across Rust frames, and a
//! Rust panic caught at the FFI boundary travels the same channel.

use crate::{Mrb, RClass, Value};

/// Error surfaced to Rust callers when mruby rejects an operation or
/// a wrapped closure panics.
#[derive(Debug, Clone)]
pub enum Error {
    /// An mruby exception captured while the VM is live. The carried
    /// `Value` is the exception object; like every `Value` it is only
    /// meaningful while the originating VM is open.
    Exception(Value),
    /// A Rust panic caught at the FFI boundary, carrying the panic
    /// payload's message. Surfaced to Rust callers (`Mrb::protect`
    /// bodies); inside a registered method the panic is re-raised to
    /// the Ruby caller as a `RuntimeError` instead.
    Panic(String),
}

impl Error {
    /// Build an exception error: a fresh instance of `class` carrying
    /// `message`, wrapped as `Error::Exception`. The magnus-aligned way
    /// a handler raises its own exception — `return Err(Error::new(...))`
    /// — formatting `message` in Rust first when it is dynamic. The
    /// bytes are copied into the exception before returning, through
    /// `RClass::exc_new`.
    #[inline]
    pub fn new(mrb: &Mrb, class: RClass, message: &str) -> Self {
        Error::Exception(class.exc_new(mrb, message))
    }

    /// The error's message. An exception renders through the live VM
    /// (the carried `Value` cannot render itself without one),
    /// falling back to an empty string when the exception's `to_s`
    /// itself fails; a panic carries its message directly.
    pub fn message(&self, mrb: &Mrb) -> String {
        match self {
            Error::Exception(exc) => exc.to_string(mrb),
            Error::Panic(msg) => msg.clone(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Exception(_) => {
                f.write_str("mruby exception (Error::message(&mrb) renders the details)")
            }
            Error::Panic(msg) => write!(f, "panic in mruby-bound closure: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Render a `catch_unwind` payload as the panic message — `&str` and
/// `String` payloads (the `panic!` macro's products) pass through,
/// anything else falls back to a fixed marker. Shared by every panic
/// boundary in the crate (`Mrb::protect`, registered methods).
#[cfg(mruby_linked)]
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(msg) => *msg,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(msg) => (*msg).to_owned(),
            Err(_) => "panic with a non-string payload".to_owned(),
        },
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;

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
    fn panic_message_renders_every_payload_shape() {
        assert_eq!(panic_message(Box::new("str payload")), "str payload");
        assert_eq!(
            panic_message(Box::new(String::from("string payload"))),
            "string payload"
        );
        assert_eq!(
            panic_message(Box::new(42_i32)),
            "panic with a non-string payload"
        );
    }
}
