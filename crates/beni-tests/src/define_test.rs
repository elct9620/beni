use crate::support::{open_mrb, same_object};
use beni::prelude::*;
use beni::{FromValue, IntoValue, Module, Mrb, Value};

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
fn module_new_takes_its_name_from_the_constant_it_is_assigned_to() {
    let mrb = open_mrb();

    // An anonymous module reaches a constant only as a value; the
    // assignment is what gives it a name.
    let module = mrb.module_new();
    mrb.object_class()
        .as_value()
        .const_set(
            &mrb,
            mrb.intern_cstr(c"BeniBoundModule")
                .expect("the name interns"),
            module.as_value(),
        )
        .expect("assigning the module to a constant must succeed");

    assert_eq!(module.name(&mrb), "BeniBoundModule");
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
        .module_get(beni::Symbol::new(&mrb, c"BeniModGet").expect("the name interns"))
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
    assert!(mrb.class_defined(beni::Symbol::new(&mrb, c"BeniDefined").expect("the name interns")));

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
        .exc_get(beni::Symbol::new(&mrb, c"ArgumentError").expect("the name interns"))
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

fn raise_own_error(mrb: &Mrb, _self: Value) -> Result<Value, beni::Error> {
    let own = mrb
        .exc_get(c"BeniRaisedError")
        .expect("the consumer's exception class is defined");
    Err(beni::Error::new(mrb, own, "raised from Rust"))
}

#[test]
fn define_error_yields_an_exception_class_ruby_code_rescues() {
    let mrb = open_mrb();
    let standard_error = mrb
        .exc_get(c"StandardError")
        .expect("StandardError is a core exception class");
    let own = mrb
        .define_error(c"BeniRaisedError", standard_error)
        .expect("defining the exception class must succeed");
    assert_eq!(own.name(&mrb), "BeniRaisedError");

    mrb.object_class()
        .define_method(&mrb, c"beni_raise", beni::method!(raise_own_error, 0))
        .expect("registering the raising method must succeed");
    let cxt = beni::Ccontext::new(&mrb, c"define_error_test.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(
            b"begin; beni_raise; rescue StandardError => e; \"#{e.class}:#{e.message}\"; end",
        )
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "the rescue must leave no pending exception: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(got.to_string(&mrb), "BeniRaisedError:raised from Rust");
}

#[test]
fn define_error_nests_under_a_namespace() {
    let mrb = open_mrb();
    let runtime_error = mrb
        .exc_get(c"RuntimeError")
        .expect("RuntimeError is a core exception class");
    let namespace = mrb
        .define_module(c"BeniErrors")
        .expect("defining the namespace must succeed");

    let nested = namespace
        .define_error(&mrb, c"ParseError", runtime_error)
        .expect("defining the nested exception class must succeed");
    assert_eq!(nested.name(&mrb), "BeniErrors::ParseError");
    assert!(nested.exc_new(&mrb, "nested").is_exception());
}

#[test]
fn define_error_fetches_a_same_named_class_and_rejects_a_conflict() {
    let mrb = open_mrb();
    let standard_error = mrb
        .exc_get(c"StandardError")
        .expect("StandardError is a core exception class");
    let runtime_error = mrb
        .exc_get(c"RuntimeError")
        .expect("RuntimeError is a core exception class");

    let first = mrb
        .define_error(c"BeniTwiceError", standard_error)
        .expect("the first definition must succeed");
    let again = mrb
        .define_error(c"BeniTwiceError", standard_error)
        .expect("the same superclass must fetch the existing class");
    assert!(same_object(&mrb, again, first));

    assert!(
        mrb.define_error(c"BeniTwiceError", runtime_error).is_err(),
        "a different superclass must surface as Err"
    );

    mrb.object_class()
        .define_const(&mrb, c"BeniNotAClass", 1i32.into_value(&mrb))
        .expect("binding the constant must succeed");
    assert!(
        mrb.define_error(c"BeniNotAClass", standard_error).is_err(),
        "a name bound to a non-class must surface as Err"
    );
}

/// `err` carries a `TypeError`.
fn assert_type_error(mrb: &Mrb, err: beni::Error) {
    match err {
        beni::Error::Exception(exc) => assert_eq!(exc.class(mrb).name(mrb), "TypeError"),
        other => panic!("the refusal must carry an exception, got {other:?}"),
    }
}

#[test]
fn define_class_fetches_a_prepended_class_as_itself() {
    let mrb = open_mrb();
    mrb.load_string(
        b"module BeniPrepender; end
          class BeniPrepended; prepend BeniPrepender; end
          module BeniPrependNs
            class Inner; prepend BeniPrepender; end
          end
          $beni_inner_before = BeniPrependNs::Inner",
    )
    .expect("the prepended classes must be defined");

    let before = mrb.class_get(c"BeniPrepended").expect("the class is bound");
    let fetched = mrb
        .define_class(c"BeniPrepended", mrb.object_class())
        .expect("the same superclass must fetch the bound class");
    assert!(same_object(&mrb, fetched, before));
    assert!(fetched.as_value().is_class());

    let ns = mrb
        .module_get(c"BeniPrependNs")
        .expect("the namespace is bound");
    let inner = ns
        .define_class(&mrb, c"Inner", mrb.object_class())
        .expect("the same superclass must fetch the nested class");
    assert!(inner.as_value().is_class());
    let unchanged = mrb
        .load_string(b"BeniPrependNs::Inner.equal?($beni_inner_before)")
        .expect("reading the constant back must succeed");
    assert_eq!(
        bool::from_value(unchanged),
        Some(true),
        "the fetch must leave the binding untouched"
    );
}

#[test]
fn define_error_on_a_prepended_exception_class_builds_its_exceptions() {
    let mrb = open_mrb();
    mrb.load_string(
        b"module BeniErrorPrepender; end
          class BeniPrependedError < StandardError; prepend BeniErrorPrepender; end",
    )
    .expect("the prepended exception class must be defined");
    let standard_error = mrb
        .exc_get(c"StandardError")
        .expect("StandardError is a core exception class");

    let fetched = mrb
        .define_error(c"BeniPrependedError", standard_error)
        .expect("the same superclass must fetch the bound class");
    let bound = mrb
        .exc_get(c"BeniPrependedError")
        .expect("the bound class is an exception class");
    assert!(same_object(&mrb, fetched, bound));

    let err = beni::Error::new(&mrb, fetched, "boom");
    assert_eq!(err.message(&mrb), "boom");
    match err {
        beni::Error::Exception(exc) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "BeniPrependedError")
        }
        other => panic!("the built error must carry an exception, got {other:?}"),
    }
}

#[test]
fn define_class_refuses_a_name_bound_to_a_singleton_class() {
    let mrb = open_mrb();
    mrb.load_string(
        b"BeniBoundSingleton = Object.new.singleton_class
          BeniBoundErrorSingleton = StandardError.new.singleton_class",
    )
    .expect("binding the singleton classes must succeed");
    let standard_error = mrb
        .exc_get(c"StandardError")
        .expect("StandardError is a core exception class");

    let err = mrb
        .define_class(c"BeniBoundSingleton", mrb.object_class())
        .expect_err("a singleton class is not an ordinary class");
    assert_type_error(&mrb, err);
    let err = mrb
        .define_error(c"BeniBoundErrorSingleton", standard_error)
        .expect_err("a singleton class is not an exception class");
    assert_type_error(&mrb, err);
}

#[test]
fn define_class_refuses_a_superclass_mismatch_as_a_type_error() {
    let mrb = open_mrb();
    let string = mrb.class_get(c"String").expect("String is a core class");
    mrb.define_class(c"BeniMismatched", mrb.object_class())
        .expect("the first definition must succeed");

    let err = mrb
        .define_class(c"BeniMismatched", string)
        .expect_err("a different superclass must be refused");
    assert_type_error(&mrb, err);
    let err = mrb
        .object_class()
        .define_class(&mrb, c"BeniMismatched", string)
        .expect_err("the namespaced form refuses it too");
    assert_type_error(&mrb, err);
}

#[test]
fn gv_get_reads_nil_for_unset_global() {
    let mrb = open_mrb();

    assert!(mrb.gv_get(c"$beni_gv_unset").is_nil());
}

#[test]
fn gv_get_observes_reassignment() {
    let mrb = open_mrb();

    // Globals are read at call time: each assignment must be
    // visible to the next read, the contract redirection-style
    // consumers (`$stdout = $stderr`) rely on.
    mrb.gv_set(c"$beni_gv", 1i32.into_value(&mrb))
        .expect("the name interns");
    assert_eq!(i32::from_value(mrb.gv_get(c"$beni_gv")), Some(1));

    mrb.gv_set(c"$beni_gv", 2i32.into_value(&mrb))
        .expect("the name interns");
    assert_eq!(i32::from_value(mrb.gv_get(c"$beni_gv")), Some(2));
}

#[test]
fn gv_remove_clears_a_global_back_to_nil() {
    let mrb = open_mrb();

    // A set global reads its value, then removing it reads nil —
    // the same as one never set.
    mrb.gv_set(c"$beni_gv_removed", 7i32.into_value(&mrb))
        .expect("the name interns");
    assert_eq!(i32::from_value(mrb.gv_get(c"$beni_gv_removed")), Some(7));

    mrb.gv_remove(c"$beni_gv_removed");
    assert!(mrb.gv_get(c"$beni_gv_removed").is_nil());

    // Removing an unset global is a no-op, not a raise.
    mrb.gv_remove(c"$beni_gv_removed");
    assert!(mrb.gv_get(c"$beni_gv_removed").is_nil());
}

#[test]
fn define_global_const_binds_a_top_level_constant() {
    let mrb = open_mrb();

    mrb.define_global_const(c"BENI_GLOBAL_ANSWER", 42i32.into_value(&mrb))
        .expect("binding a top-level constant must succeed");

    let got = mrb
        .load_string(b"BENI_GLOBAL_ANSWER")
        .expect("the constant reads back from Ruby");
    assert_eq!(i64::from_value(got), Some(42));
}

#[test]
fn define_global_const_surfaces_a_frozen_object_as_err() {
    let mrb = open_mrb();
    mrb.load_string(b"Object.freeze")
        .expect("freezing Object must succeed");
    let frozen_error = mrb.exc_get(c"FrozenError").expect("a core exception class");

    let err = mrb
        .define_global_const(c"BENI_GLOBAL_ON_FROZEN", Value::nil())
        .expect_err("binding onto a frozen Object must surface as Err");

    assert!(err.is_kind_of(&mrb, frozen_error));
    assert!(
        mrb.pending_exc().is_nil(),
        "the caught exception must not stay pending"
    );
}

#[test]
fn a_global_answers_its_absent_value_for_a_key_too_long_to_intern() {
    let mrb = open_mrb();
    let name =
        std::ffi::CString::new(vec![b'a'; u16::MAX as usize]).expect("the name holds no NUL");

    // The key names no symbol, so the read answers nil and the removal
    // does nothing, while the assignment — which has an `Err` to carry
    // it in — surfaces the intern's own.
    assert!(mrb.gv_get(name.as_c_str()).is_nil());
    mrb.gv_remove(name.as_c_str());
    let err = mrb
        .gv_set(name.as_c_str(), Value::nil())
        .expect_err("the assignment must refuse the name");
    let argument_error = mrb
        .exc_get(c"ArgumentError")
        .expect("ArgumentError is built in");
    assert!(err.is_kind_of(&mrb, argument_error));
}
