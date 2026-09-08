//! The `Gem` trait — the unit of Ruby surface a Rust crate ships.
//!
//! A crate exposing classes, modules, or methods to mruby implements
//! `Gem` and performs every definition inside `init`. The embedder
//! installs each gem during interpreter setup through
//! `Mrb::init_gem`, which owns the panic boundary: a panic inside an
//! `init` body surfaces as `Err(Error::Panic)` to the embedder
//! instead of unwinding further, and an `Err` from `init` aborts the
//! setup by surfacing as-is.

use crate::{Error, Mrb};

/// The unit of Ruby surface a Rust crate ships. Implementations
/// define their classes, modules, and methods against the live
/// interpreter handle; Ruby-level rejections arrive as `Err` from
/// the definition APIs and propagate out of `init` naturally.
pub trait Gem {
    /// Install this gem's Ruby surface. Invoked by the embedder via
    /// `Mrb::init_gem` during interpreter setup; an `Err` aborts the
    /// setup and surfaces to the embedder.
    fn init(mrb: &Mrb) -> Result<(), Error>;
}

impl Mrb {
    /// Install `G`'s Ruby surface during interpreter setup. Returns
    /// `init`'s own `Err` unchanged (aborting the setup is the
    /// embedder's move), and converts a panic inside the `init` body
    /// into `Err(Error::Panic)` so it never unwinds past the wrapper.
    pub fn init_gem<G: Gem>(&self) -> Result<(), Error> {
        // The panic boundary for `Gem::init` bodies: catching
        // here keeps the unwind inside the wrapper. The closure
        // only borrows `self`, so no observable broken state
        // survives the catch (AssertUnwindSafe as in
        // `Mrb::protect`).
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| G::init(self))) {
            Ok(result) => result,
            Err(payload) => Err(Error::Panic(crate::error::panic_message(payload))),
        }
    }
}
