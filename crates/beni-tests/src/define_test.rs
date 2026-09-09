use crate::support::open_mrb;
use beni::{FromValue, Module, Mrb, Value};

fn answer_seven(_mrb: &Mrb, _self: Value) -> i32 {
    7
}

#[test]
fn class_new_creates_an_unnamed_usable_class() {
    let mrb = open_mrb();

    // An anonymous class inherits from the given superclass, carries
    // no name until bound to a constant, yet is fully usable through
    // the returned handle: a method registered on it is callable on
    // an instance.
    let class = mrb
        .class_new(mrb.object_class())
        .expect("creating an anonymous class under Object must succeed");
    // An unbound class has no constant path: mruby synthesizes an
    // `#<Class:0x..>` name rather than a real one.
    assert!(
        class.name(&mrb).starts_with("#<Class:"),
        "the class must be unnamed: {:?}",
        class.name(&mrb)
    );

    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering a method on the anonymous class must succeed");
    let got = class
        .obj_new(&mrb, &[])
        .expect("the anonymous class instantiates")
        .funcall(&mrb, c"answer", &[])
        .expect("the method on the anonymous class must be callable");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn class_new_surfaces_err_for_a_rejected_superclass() {
    let mrb = open_mrb();

    // mruby rejects `Class` itself as a superclass; the typed form
    // catches the raise instead of long-jumping across FFI.
    let class_class = mrb
        .class_get(c"Class")
        .expect("Class must resolve to its class object");
    assert!(
        mrb.class_new(class_class).is_err(),
        "a rejected superclass must surface as Err"
    );
}

#[test]
fn module_new_creates_an_unnamed_mixable_module() {
    let mrb = open_mrb();

    // An anonymous module carries no name, yet mixing it into a class
    // makes its method reachable on that class's instances.
    let module = mrb.module_new();
    // An unbound module has no constant path: mruby synthesizes an
    // `#<Module:0x..>` name rather than a real one.
    assert!(
        module.name(&mrb).starts_with("#<Module:"),
        "the module must be unnamed: {:?}",
        module.name(&mrb)
    );
    module
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering a method on the anonymous module must succeed");

    let class = mrb
        .class_new(mrb.object_class())
        .expect("creating the host class must succeed");
    class
        .include_module(&mrb, module)
        .expect("mixing the anonymous module in must succeed");
    let got = class
        .obj_new(&mrb, &[])
        .expect("the host class instantiates")
        .funcall(&mrb, c"answer", &[])
        .expect("the mixed-in method must be reachable");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn module_get_fetches_a_defined_module() {
    let mrb = open_mrb();

    // A top-level module is fetchable by name and by Symbol key —
    // both forms route through `mrb_module_get_id`.
    mrb.define_module(c"BeniModGet")
        .expect("defining the module must succeed");
    let by_name = mrb
        .module_get(c"BeniModGet")
        .expect("fetching by name must reach the defined module");
    assert_eq!(by_name.name(&mrb), "BeniModGet");
    let by_sym = mrb
        .module_get(beni::Symbol::new(&mrb, c"BeniModGet"))
        .expect("fetching by Symbol key must reach the defined module");
    assert_eq!(by_sym.name(&mrb), "BeniModGet");
}

#[test]
fn module_get_surfaces_name_error_for_missing_module() {
    let mrb = open_mrb();

    // mruby raises NameError for a missing constant (vendored
    // src/class.c documents the lookup contract) — the typed
    // lookup must catch it instead of long-jumping.
    let err = mrb
        .module_get(c"BeniNoSuchModule")
        .expect_err("missing module must surface as Err");
    assert!(
        err.message(&mrb).contains("BeniNoSuchModule"),
        "the NameError must name the missing constant: {}",
        err.message(&mrb)
    );
}

#[test]
fn class_defined_answers_a_total_bool_for_top_level_names() {
    let mrb = open_mrb();

    // A defined top-level class reads `true` by name and by Symbol
    // key — both route through `mrb_class_defined_id`.
    mrb.define_class(c"BeniDefined", mrb.object_class())
        .expect("defining the class must succeed");
    assert!(mrb.class_defined(c"BeniDefined"));
    assert!(mrb.class_defined(beni::Symbol::new(&mrb, c"BeniDefined")));

    // An undefined name reads `false` instead of raising — the
    // predicate is total.
    assert!(!mrb.class_defined(c"BeniNeverDefined"));
    assert!(mrb.pending_exc().is_nil(), "the predicate must not raise");
}

#[test]
fn exc_get_fetches_a_builtin_exception_class() {
    let mrb = open_mrb();

    // A built-in exception class is reachable by name and by Symbol
    // key — both forms route through `mrb_exc_get_id`.
    let by_name = mrb
        .exc_get(c"RuntimeError")
        .expect("RuntimeError must resolve to its exception class");
    assert_eq!(by_name.name(&mrb), "RuntimeError");
    let by_sym = mrb
        .exc_get(beni::Symbol::new(&mrb, c"ArgumentError"))
        .expect("a Symbol key must reach the exception class");
    assert_eq!(by_sym.name(&mrb), "ArgumentError");
}

#[test]
fn exc_get_surfaces_err_for_a_non_exception_class() {
    let mrb = open_mrb();

    // `Object` is a class but not an Exception subclass; the
    // Exception-subclass guarantee turns this into an Err instead of
    // long-jumping.
    assert!(
        mrb.exc_get(c"Object").is_err(),
        "a non-exception class must surface as Err"
    );
}

#[test]
fn exc_get_surfaces_err_for_missing_constant() {
    let mrb = open_mrb();

    // mruby raises NameError for a missing constant — the typed
    // lookup must catch it instead of long-jumping.
    assert!(
        mrb.exc_get(c"BeniNoSuchError").is_err(),
        "a missing constant must surface as Err"
    );
}

#[test]
fn gv_get_reads_nil_for_unset_global() {
    let mrb = open_mrb();

    let sym = mrb.intern_cstr(c"$beni_gv_unset");

    assert!(mrb.gv_get(sym).is_nil());
}

#[test]
fn gv_get_observes_reassignment() {
    let mrb = open_mrb();
    let sym = mrb.intern_cstr(c"$beni_gv");

    // Globals are read at call time: each assignment must be
    // visible to the next read, the contract redirection-style
    // consumers (`$stdout = $stderr`) rely on.
    mrb.gv_set(sym, Value::from_int(&mrb, 1));
    assert_eq!(i32::from_value(mrb.gv_get(sym)), Some(1));

    mrb.gv_set(sym, Value::from_int(&mrb, 2));
    assert_eq!(i32::from_value(mrb.gv_get(sym)), Some(2));
}

#[test]
fn gv_remove_clears_a_global_back_to_nil() {
    let mrb = open_mrb();
    let sym = mrb.intern_cstr(c"$beni_gv_removed");

    // A set global reads its value, then removing it reads nil —
    // the same as one never set.
    mrb.gv_set(sym, Value::from_int(&mrb, 7));
    assert_eq!(i32::from_value(mrb.gv_get(sym)), Some(7));

    mrb.gv_remove(sym);
    assert!(mrb.gv_get(sym).is_nil());

    // Removing an unset global is a no-op, not a raise.
    mrb.gv_remove(sym);
    assert!(mrb.gv_get(sym).is_nil());
}
