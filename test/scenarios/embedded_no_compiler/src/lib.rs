//! A consumer's Ruby surface for a target that never compiles Ruby.
//!
//! The gem is defined from Rust and reached from Rust, so nothing here
//! needs a compiler at run time. What the scenario pins is that this
//! whole path works against an archive built without mruby's compiler
//! gem, and against a `beni` with its default features off.
//!
//! The compile surface is not reachable in that build:
//!
//! ```compile_fail
//! let _ = beni::Ccontext::new;
//! ```
//!
//! ```compile_fail
//! let _ = beni::Mrb::load_string;
//! ```

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
