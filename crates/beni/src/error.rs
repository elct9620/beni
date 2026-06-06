//! The `Error` type surfaced across the mruby boundary.
//!
//! Mirrors the role of `magnus::Error`: every safe operation that
//! mruby can reject — class/module definition, method registration,
//! constant lookup — returns `Result<_, Error>` instead of letting
//! the raise long-jump across Rust frames. The raise is caught by
//! `Mrb::protect` inside the wrapper, so the error carries the
//! exception object mruby produced.

use crate::{Mrb, Value};

/// Error surfaced to Rust callers when mruby rejects an operation.
#[derive(Debug, Clone, Copy)]
pub enum Error {
    /// An mruby exception captured while the VM is live. The carried
    /// `Value` is the exception object; like every `Value` it is only
    /// meaningful while the originating VM is open.
    Exception(Value),
}

impl Error {
    /// The exception's message, extracted through the live VM (the
    /// carried exception `Value` cannot render itself without one).
    /// Falls back to an empty string when the exception's `to_s`
    /// itself fails.
    pub fn message(&self, mrb: &Mrb) -> String {
        match self {
            Error::Exception(exc) => exc.to_string(mrb),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Exception(_) => {
                f.write_str("mruby exception (Error::message(&mrb) renders the details)")
            }
        }
    }
}

impl std::error::Error for Error {}
