//! The `Error` type surfaced across the mruby boundary.
//!
//! Mirrors the role of `magnus::Error`: every safe operation that
//! mruby can reject — class/module definition, method registration,
//! constant lookup, protected execution — returns `Result<_, Error>`
//! instead of letting the raise long-jump across Rust frames, and a
//! Rust panic caught at the FFI boundary travels the same channel.

use crate::{Mrb, ParseMessage, RClass, Value};
use beni_sys as sys;

/// Error surfaced to Rust callers when mruby rejects an operation or
/// a wrapped closure panics.
#[derive(Debug, Clone)]
pub enum Error {
    /// An mruby exception captured while the VM is live. The carried
    /// `Value` is the exception object; like every `Value` it is only
    /// meaningful while the originating VM is open.
    Exception(Value),
    /// Source that did not parse, carrying the compiler's first
    /// recorded diagnostic. Distinct from `Exception` because a
    /// program that never compiled never ran: there is no exception
    /// object and no backtrace behind it.
    Syntax(ParseMessage),
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

    /// Build the canonical `ArgumentError` for a wrong argument count —
    /// the magnus-aligned way a handler validating its own arity reports
    /// it: `return Err(Error::argnum(mrb, given, min, max))`. `min == max`
    /// renders "expected `min`", a negative `max` renders "expected
    /// `min`+", and `min < max` renders "expected `min`..`max`". The
    /// message comes from mruby's own `mrb_argnum_error`, captured
    /// through `Mrb::protect` so its raise surfaces as the returned
    /// `Error::Exception` instead of long-jumping. `given` saturates to
    /// `sys::mrb_int::MAX` (the archive's configured integer width), like
    /// `Mrb::str_new`; real argument counts stay far below that.
    #[inline]
    pub fn argnum(mrb: &Mrb, given: i64, min: i32, max: i32) -> Self {
        let argc = given.min(sys::mrb_int::MAX as i64) as sys::mrb_int;
        match mrb.protect(|mrb| {
            // SAFETY: `mrb` is alive inside the protect frame;
            // `mrb_argnum_error` raises `ArgumentError`, caught by
            // `protect` and surfaced as the `Err` below.
            unsafe {
                sys::mrb_argnum_error(
                    mrb.as_ptr(),
                    argc,
                    min as core::ffi::c_int,
                    max as core::ffi::c_int,
                );
            }
        }) {
            Err(err) => err,
            // `mrb_argnum_error` always raises, so `protect` returns
            // `Err`; an `Ok` would mean the symbol did not raise.
            Ok(_) => unreachable!("mrb_argnum_error must raise ArgumentError"),
        }
    }

    /// The frames the carried exception's `backtrace` answers, each
    /// rendered as mruby writes it (`file:line:in method`).
    ///
    /// A `Syntax` or `Panic` error answers an empty list, and so does
    /// an exception holding no backtrace — one raised under no compile
    /// context never got the `debug_info` a backtrace is packed from.
    /// Reading the backtrace runs Ruby, so an exception whose
    /// `backtrace` is overridden and raises answers an empty list too,
    /// the way `Value::to_string` renders a raising `to_s`.
    pub fn backtrace(&self, mrb: &Mrb) -> Vec<String> {
        let Error::Exception(exc) = self else {
            return Vec::new();
        };
        // `ensure_array` rejects the `nil` an exception with no
        // backtrace answers, so the absent case needs no test of its
        // own.
        let Ok(frames) = exc
            .funcall(mrb, c"backtrace", &[])
            .and_then(|frames| frames.ensure_array(mrb))
        else {
            return Vec::new();
        };
        frames
            .entries()
            .map(|frame| frame.string_lossy(mrb))
            .collect()
    }

    /// The error's message. An exception renders through the live VM
    /// (the carried `Value` cannot render itself without one),
    /// falling back to an empty string when the exception's `to_s`
    /// itself fails; a syntax error and a panic each carry their
    /// message directly.
    pub fn message(&self, mrb: &Mrb) -> String {
        match self {
            Error::Exception(exc) => exc.to_string(mrb),
            Error::Syntax(parse) => parse.message().to_owned(),
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
            Error::Syntax(parse) => write!(
                f,
                "syntax error at line {}, column {}: {}",
                parse.line(),
                parse.column(),
                parse.message()
            ),
            Error::Panic(msg) => write!(f, "panic in mruby-bound closure: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Render a `catch_unwind` payload as the panic message — `&str` and
/// `String` payloads (the `panic!` macro's products) pass through,
/// anything else falls back to a fixed marker. Shared by every panic
/// boundary in the crate (`Mrb::protect`, registered methods).
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(msg) => *msg,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(msg) => (*msg).to_owned(),
            Err(_) => "panic with a non-string payload".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
