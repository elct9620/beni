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
        #[cfg(mruby_linked)]
        {
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
        #[cfg(not(mruby_linked))]
        crate::not_linked()
    }
}

#[cfg(all(test, mruby_linked))]
mod tests {
    use super::*;
    use crate::{method, FromValue, Module, Value};

    fn answer(_mrb: &Mrb, _self: Value) -> i32 {
        42
    }

    /// A complete gem surface: module + class + method, all defined
    /// inside `init`.
    struct WidgetGem;

    impl Gem for WidgetGem {
        fn init(mrb: &Mrb) -> Result<(), Error> {
            let outer = mrb.define_module(c"BeniGem")?;
            let class = outer.define_class(mrb, c"Widget", mrb.object_class())?;
            class.define_method(mrb, c"answer", method!(answer, 0))?;
            Ok(())
        }
    }

    /// A gem whose `init` reports failure — stands in for any
    /// definition mruby rejects.
    struct RefusingGem;

    impl Gem for RefusingGem {
        fn init(mrb: &Mrb) -> Result<(), Error> {
            // A real rejection: redefining Object's superclass
            // cannot succeed, so surface the resulting Err.
            let object = mrb.object_class();
            let base = mrb.define_class(c"BeniGemBase", object)?;
            mrb.define_class(c"BeniGemBase", base).map(|_| ())
        }
    }

    /// A gem whose `init` panics — the boundary must convert it.
    struct PanickingGem;

    impl Gem for PanickingGem {
        fn init(_mrb: &Mrb) -> Result<(), Error> {
            panic!("gem init went sideways");
        }
    }

    #[test]
    fn init_gem_installs_the_gem_surface() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        mrb.init_gem::<WidgetGem>()
            .expect("installing the gem must succeed");

        // The surface must be reachable from plain Ruby source — the
        // embedder-shaped journey end to end.
        let cxt = crate::Ccontext::new(&mrb, c"gem_test.rb")
            .expect("allocating the compile context must succeed");
        let got = cxt.load_nstring(b"BeniGem::Widget.new.answer");
        assert!(
            mrb.pending_exc().is_nil(),
            "evaluating the gem surface must not raise: {}",
            mrb.pending_exc().to_string(&mrb)
        );
        assert_eq!(i32::from_value(got), Some(42));
    }

    #[test]
    fn init_gem_surfaces_init_err_to_the_embedder() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .init_gem::<RefusingGem>()
            .expect_err("the init failure must surface");
        assert!(
            err.message(&mrb).contains("superclass mismatch"),
            "the embedder must see init's own error: {}",
            err.message(&mrb)
        );
    }

    #[test]
    fn init_gem_catches_init_panic() {
        let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

        let err = mrb
            .init_gem::<PanickingGem>()
            .expect_err("the panic must surface as Err");
        match err {
            Error::Panic(msg) => assert!(msg.contains("gem init went sideways")),
            Error::Exception(_) => panic!("an init panic must surface as Error::Panic"),
        }
    }
}
