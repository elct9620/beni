use beni::{Error, FromValue, Gem, Module, Mrb, Value};

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
        class.define_method(mrb, c"answer", beni::method!(answer, 0))?;
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
    let cxt = beni::Ccontext::new(&mrb, c"gem_test.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(b"BeniGem::Widget.new.answer")
        .expect("the test source must compile and run");
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
        other => panic!("an init panic must surface as Error::Panic, got {other}"),
    }
}
