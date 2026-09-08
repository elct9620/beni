//! A consumer's Ruby surface: one gem, one class, one method.
//!
//! Small on purpose — the scenario is about whether a crate outside
//! the workspace can find, link, and run the archive `beni:build`
//! staged here, not about the breadth of the API. That breadth is
//! `beni-tests`.

use beni::{Error, Gem, Module, Mrb, Value};

fn double(_mrb: &Mrb, _self: Value, n: i32) -> i32 {
    n * 2
}

/// The gem an embedder installs through `Mrb::init_gem`.
pub struct Doubler;

impl Gem for Doubler {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        let class = mrb.define_class(c"Doubler", mrb.object_class())?;
        class.define_method(mrb, c"call", beni::method!(double, 1))?;
        Ok(())
    }
}

#[cfg(test)]
mod consumer_test;
