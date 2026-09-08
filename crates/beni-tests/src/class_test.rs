use beni::{Error, IntoSym, Module, Mrb, Object, RClass, Value};

/// Registration target answering a fixed Integer for the trait
/// tests below.
fn answer_seven(_mrb: &Mrb, _self: Value) -> i32 {
    7
}

fn answer_nine(_mrb: &Mrb, _self: Value) -> i32 {
    9
}

#[test]
fn symbol_key_reaches_the_same_definition_as_the_name() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // Defining with an already-interned Symbol must reach the same
    // definition path as a name key: the class is then fetchable by
    // its plain name, and a method registered under a Symbol key is
    // callable.
    let name = beni::Symbol::new(&mrb, c"BeniSymKeyed");
    let class = mrb
        .define_class(name, object)
        .expect("defining the class under a Symbol key must succeed");
    class
        .define_method(
            &mrb,
            beni::Symbol::new(&mrb, c"answer"),
            beni::method!(answer_seven, 0),
        )
        .expect("registering a method under a Symbol key must succeed");
    class
        .define_const(
            &mrb,
            beni::Symbol::new(&mrb, c"ANSWER"),
            Value::from_int(&mrb, 7),
        )
        .expect("binding a constant under a Symbol key must succeed");

    // Fetch by Symbol key resolves to the class defined above.
    let fetched = mrb
        .class_get(beni::Symbol::new(&mrb, c"BeniSymKeyed"))
        .expect("fetching by a Symbol key must reach the defined class");
    assert_eq!(fetched.name(&mrb), "BeniSymKeyed");

    // The method and constant keyed by Symbol read back through the
    // equivalent name — both keys resolve to the same interned sym.
    let receiver = fetched
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"answer", &[])
        .expect("the Symbol-keyed method must be callable by name");
    assert_eq!(unsafe { got.unbox_integer() }, 7);

    // SAFETY: `fetched` is a live handle from this VM.
    let const_val = unsafe { fetched.to_value(&mrb) }
        .const_get(&mrb, mrb.intern_cstr(c"ANSWER"))
        .expect("the Symbol-keyed constant must read by name");
    assert_eq!(unsafe { const_val.unbox_integer() }, 7);
}

#[test]
fn symbol_key_and_name_key_are_interchangeable_for_lookup() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // A class defined by name is fetchable by Symbol, and one defined
    // by Symbol is fetchable by name — the two key forms are
    // interchangeable because both resolve to the same interned sym.
    mrb.define_class(c"BeniByName", object)
        .expect("defining by name must succeed");
    let by_sym = mrb
        .class_get(beni::Symbol::new(&mrb, c"BeniByName"))
        .expect("a name-defined class is fetchable by Symbol key");
    assert_eq!(by_sym.name(&mrb), "BeniByName");
}

#[test]
fn symbol_key_registers_private_singleton_and_module_function() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // Each registration variant routes its Symbol key through the
    // matching `_id` C function; a method registered under a Symbol
    // is then reachable, proving the key reached the definition.
    let class = mrb
        .define_class(c"BeniSymVariants", object)
        .expect("defining the class must succeed");
    class
        .define_private_method(
            &mrb,
            beni::Symbol::new(&mrb, c"secret"),
            beni::method!(answer_seven, 0),
        )
        .expect("registering the private method under a Symbol key must succeed");
    class
        .define_singleton_method(
            &mrb,
            beni::Symbol::new(&mrb, c"klass_answer"),
            beni::method!(answer_nine, 0),
        )
        .expect("registering the singleton method under a Symbol key must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    // funcall bypasses visibility, reaching the private body.
    let private = receiver
        .funcall(&mrb, c"secret", &[])
        .expect("the Symbol-keyed private method must be reachable via funcall");
    assert_eq!(unsafe { private.unbox_integer() }, 7);
    // SAFETY: `class` is a live handle from this VM.
    let singleton = unsafe { class.to_value(&mrb) }
        .funcall(&mrb, c"klass_answer", &[])
        .expect("the Symbol-keyed singleton method must be callable");
    assert_eq!(unsafe { singleton.unbox_integer() }, 9);

    let module = mrb
        .define_module(c"BeniSymModFn")
        .expect("defining the module must succeed");
    module
        .define_module_function(
            &mrb,
            beni::Symbol::new(&mrb, c"mod_seven"),
            beni::method!(answer_seven, 0),
        )
        .expect("registering the module function under a Symbol key must succeed");

    // The module function is callable as a singleton on the module
    // object — the consumer-visible end of the Symbol-keyed
    // registration.
    let cxt = beni::Ccontext::new(&mrb, c"sym_modfn.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(b"BeniSymModFn.mod_seven")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "calling the Symbol-keyed module function must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(unsafe { got.unbox_integer() }, 7);
}

#[test]
fn nested_definition_accepts_a_symbol_key() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // The namespaced Module-trait define/get also accept a Symbol key.
    let outer = mrb
        .define_module(beni::Symbol::new(&mrb, c"BeniSymNs"))
        .expect("defining the module under a Symbol key must succeed");
    let nested = outer
        .define_class(&mrb, beni::Symbol::new(&mrb, c"Inner"), object)
        .expect("defining the nested class under a Symbol key must succeed");
    assert_eq!(nested.name(&mrb), "BeniSymNs::Inner");

    let fetched = outer
        .class_get(&mrb, beni::Symbol::new(&mrb, c"Inner"))
        .expect("fetching the nested class under a Symbol key must succeed");
    assert_eq!(fetched.name(&mrb), "BeniSymNs::Inner");
}

#[test]
fn define_class_surfaces_mruby_rejection_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let base = mrb
        .define_class(c"BeniErrBase", object)
        .expect("defining the base class must succeed");
    mrb.define_class(c"BeniErrChild", base)
        .expect("defining the child class must succeed");

    // Redefining with a different superclass is the documented
    // E_TYPE_ERROR rejection (vendored src/class.c superclass
    // mismatch) — it must surface as Err, not a longjmp.
    let err = mrb
        .define_class(c"BeniErrChild", object)
        .expect_err("superclass mismatch must surface as Err");
    assert!(matches!(err, Error::Exception(_)));
    assert!(
        err.message(&mrb).contains("superclass mismatch"),
        "unexpected rejection message: {}",
        err.message(&mrb)
    );
}

#[test]
fn class_get_surfaces_name_error_for_missing_class() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // mruby raises NameError for a missing constant (vendored
    // src/class.c documents the lookup contract) — the typed
    // lookup must catch it instead of long-jumping.
    let err = mrb
        .class_get(c"BeniNoSuchClass")
        .expect_err("missing class must surface as Err");
    assert!(
        err.message(&mrb).contains("BeniNoSuchClass"),
        "the NameError must name the missing constant: {}",
        err.message(&mrb)
    );
}

#[test]
fn module_get_fetches_a_nested_module_by_either_key() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // The namespaced module lookup mirrors the nested class_get:
    // a name key and a Symbol key both route through
    // `mrb_module_get_under_id` and resolve to the same module.
    let outer = mrb
        .define_module(c"BeniModNs")
        .expect("defining the outer module must succeed");
    let nested = outer
        .define_module(&mrb, c"Inner")
        .expect("defining the nested module must succeed");
    assert_eq!(nested.name(&mrb), "BeniModNs::Inner");

    let by_name = outer
        .module_get(&mrb, c"Inner")
        .expect("fetching the nested module by name must succeed");
    let by_sym = outer
        .module_get(&mrb, beni::Symbol::new(&mrb, c"Inner"))
        .expect("fetching the nested module by Symbol key must succeed");
    assert_eq!(by_name.name(&mrb), "BeniModNs::Inner");
    assert_eq!(by_name.as_raw(), by_sym.as_raw());
}

#[test]
fn module_get_surfaces_err_for_missing_and_non_module() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let outer = mrb
        .define_module(c"BeniModNsErr")
        .expect("defining the outer module must succeed");

    // mruby raises NameError for a missing constant — caught into
    // Err instead of long-jumping.
    let missing = outer
        .module_get(&mrb, c"BeniNoSuchModule")
        .expect_err("a missing nested module must surface as Err");
    assert!(matches!(missing, Error::Exception(_)));

    // A nested class is not a module: mruby raises TypeError, also
    // surfaced as Err.
    outer
        .define_class(&mrb, c"NotAModule", object)
        .expect("defining the nested class must succeed");
    let not_module = outer
        .module_get(&mrb, c"NotAModule")
        .expect_err("a non-module constant must surface as Err");
    assert!(matches!(not_module, Error::Exception(_)));
}

#[test]
fn class_defined_answers_a_total_bool_within_a_namespace() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let outer = mrb
        .define_module(c"BeniDefinedNs")
        .expect("defining the outer module must succeed");
    outer
        .define_class(&mrb, c"Inner", object)
        .expect("defining the nested class must succeed");

    // A defined nested name reads `true` by name and by Symbol key —
    // both route through `mrb_class_defined_under_id`.
    assert!(outer.class_defined(&mrb, c"Inner"));
    assert!(outer.class_defined(&mrb, beni::Symbol::new(&mrb, c"Inner")));

    // An undefined nested name reads `false` instead of raising — the
    // predicate is total.
    assert!(!outer.class_defined(&mrb, c"NoSuchInner"));
    assert!(mrb.pending_exc().is_nil(), "the predicate must not raise");
}

#[test]
fn obj_new_surfaces_a_raising_initialize_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt =
        beni::Ccontext::new(&mrb, c"obj_new_test.rb").expect("allocating the context must succeed");

    cxt.load_nstring(b"class BeniBoomInit; def initialize; raise 'no'; end; end")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise"
    );

    // Constructing runs the raising initialize — surfaced as Err
    // instead of long-jumping across the call.
    let class = mrb
        .class_get(c"BeniBoomInit")
        .expect("the class is defined");
    assert!(matches!(class.obj_new(&mrb, &[]), Err(Error::Exception(_))));
}

#[test]
fn registering_onto_a_frozen_class_surfaces_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt =
        beni::Ccontext::new(&mrb, c"frozen_reg.rb").expect("allocating the context must succeed");

    // The handle still resolves once the class is frozen, but
    // registering onto it raises FrozenError — caught into Err the
    // same way the other definition rejections are.
    cxt.load_nstring(b"class BeniFrozenReg; end; BeniFrozenReg.freeze")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining and freezing must not raise"
    );
    let class = mrb
        .class_get(c"BeniFrozenReg")
        .expect("the class is defined");

    assert!(matches!(
        class.define_method(&mrb, c"m", beni::method!(answer_seven, 0)),
        Err(Error::Exception(_))
    ));
}

#[test]
fn private_method_rejects_public_dispatch_but_is_attached() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniPrivate", object)
        .expect("defining the class must succeed");
    class
        .define_private_method(&mrb, c"secret", beni::method!(answer_seven, 0))
        .expect("registering the private method must succeed");
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // VM-dispatched code with an explicit receiver must observe
    // the visibility (the funcall path bypasses it by design):
    // OP_SEND raises NoMethodError for a private method.
    // SAFETY: `mrb` is alive; the code literal is NUL-terminated.
    // `mrb_load_string` absorbs the raise into `mrb->exc`.
    let _ = unsafe { beni::sys::mrb_load_string(mrb.as_ptr(), c"BeniPrivate.new.secret".as_ptr()) };
    let exc = mrb.pending_exc();
    assert!(
        !exc.is_nil(),
        "public dispatch of a private method must raise"
    );
    let message = Error::Exception(exc).message(&mrb);
    assert!(
        message.contains("private"),
        "the NoMethodError must name the visibility: {message}"
    );
    mrb.clear_exc();

    // mrb_funcall bypasses visibility, confirming the body is
    // attached and runs.
    let got = receiver
        .funcall(&mrb, c"secret", &[])
        .expect("funcall dispatch must reach the private body");
    assert_eq!(unsafe { got.unbox_integer() }, 7);
}

#[test]
fn alias_method_keys_both_names_as_symbol_or_name() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniAliasKeyed", object)
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the original method must succeed");

    // Both names accept a symbol-or-name key independently: alias once
    // with a Symbol new-name against a name old-name, and once with both
    // as name keys. Each alias must reach the same body as the original.
    class
        .alias_method(&mrb, beni::Symbol::new(&mrb, c"by_sym"), c"answer")
        .expect("aliasing under a Symbol new-name key must succeed");
    class
        .alias_method(&mrb, c"by_name", c"answer")
        .expect("aliasing under a name key must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let original = receiver
        .funcall(&mrb, c"answer", &[])
        .expect("the original method must be callable");
    let by_sym = receiver
        .funcall(&mrb, c"by_sym", &[])
        .expect("the Symbol-keyed alias must be callable");
    let by_name = receiver
        .funcall(&mrb, c"by_name", &[])
        .expect("the name-keyed alias must be callable");
    assert_eq!(unsafe { original.unbox_integer() }, 7);
    assert_eq!(unsafe { by_sym.unbox_integer() }, 7);
    assert_eq!(unsafe { by_name.unbox_integer() }, 7);
}

#[test]
fn module_and_object_traits_register_methods() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // Module trait: nested definition + instance-method
    // registration, exercised end-to-end through a Ruby call.
    let outer = mrb
        .define_module(c"BeniTrait")
        .expect("defining the module must succeed");
    let class = outer
        .define_class(&mrb, c"Widget", object)
        .expect("defining the nested class must succeed");
    assert_eq!(class.name(&mrb), "BeniTrait::Widget");

    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the instance method must succeed");
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"answer", &[])
        .expect("the registered method must not raise");
    assert_eq!(unsafe { got.unbox_integer() }, 7);

    // Object trait: singleton registration on the class handle,
    // invoked through the reified class value.
    class
        .define_singleton_method(&mrb, c"class_answer", beni::method!(answer_nine, 0))
        .expect("registering the singleton method must succeed");
    // SAFETY: `class` is a live handle from this VM.
    let class_value = unsafe { class.to_value(&mrb) };
    let got = class_value
        .funcall(&mrb, c"class_answer", &[])
        .expect("the registered class method must not raise");
    assert_eq!(unsafe { got.unbox_integer() }, 9);

    // Lookup round-trip through the trait.
    let fetched = outer
        .class_get(&mrb, c"Widget")
        .expect("fetching the nested class must succeed");
    assert_eq!(fetched.name(&mrb), "BeniTrait::Widget");
}

#[test]
fn define_module_function_attaches_to_module_and_includers() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let module = mrb
        .define_module(c"BeniModFn")
        .expect("defining the module must succeed");
    module
        .define_module_function(&mrb, c"seven", beni::method!(answer_seven, 0))
        .expect("registering the module function must succeed");

    let cxt = beni::Ccontext::new(&mrb, c"modfn_test.rb")
        .expect("allocating the compile context must succeed");

    // Callable directly on the module object — the singleton form.
    let direct = cxt
        .load_nstring(b"BeniModFn.seven")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "calling the module function must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(unsafe { direct.unbox_integer() }, 7);

    // Callable as a bare private helper inside a class that mixes the
    // module in — the private-instance form.
    let included = cxt
        .load_nstring(
            b"class BeniModUser; include BeniModFn; def go; seven; end; end; BeniModUser.new.go",
        )
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "calling the mixed-in private form must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(unsafe { included.unbox_integer() }, 7);
}

#[test]
fn module_function_instance_form_is_private() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let module = mrb
        .define_module(c"BeniModFnPriv")
        .expect("defining the module must succeed");
    module
        .define_module_function(&mrb, c"seven", beni::method!(answer_seven, 0))
        .expect("registering the module function must succeed");

    let cxt = beni::Ccontext::new(&mrb, c"modfn_priv_test.rb")
        .expect("allocating the compile context must succeed");
    cxt.load_nstring(b"class BeniModUserPriv; include BeniModFnPriv; end")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the includer must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // The mixed-in instance form is private: dispatching it with an
    // explicit receiver raises NoMethodError — the visibility half of
    // `module_function` the bare-helper call alone cannot prove.
    let err = cxt
        .load_nstring(b"BeniModUserPriv.new.seven")
        .expect_err("explicit-receiver dispatch of the private instance form must raise");
    let message = err.message(&mrb);
    assert!(
        message.contains("private"),
        "the NoMethodError must name the visibility: {message}"
    );
    mrb.clear_exc();
}

#[test]
fn define_const_binds_a_constant_readable_from_ruby() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let module = mrb
        .define_module(c"BeniConstHost")
        .expect("defining the host module must succeed");

    module
        .define_const(&mrb, c"ANSWER", Value::from_int(&mrb, 42))
        .expect("binding the constant must succeed");

    // The constant must resolve from plain Ruby source — the
    // consumer-visible end of the binding.
    let cxt = beni::Ccontext::new(&mrb, c"const_test.rb")
        .expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(b"BeniConstHost::ANSWER")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "reading the constant must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert!(got.is_integer(), "the bound constant reads back as Integer");
    assert_eq!(unsafe { got.unbox_integer() }, 42);
}

#[test]
fn alias_method_binds_a_second_name_for_an_existing_method() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniAlias", object)
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the original method must succeed");

    class
        .alias_method(&mrb, c"original_answer", c"answer")
        .expect("aliasing an existing method must succeed");

    // The alias resolves to the same body as the original — the
    // consumer-visible point of preserving a method before override.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"original_answer", &[])
        .expect("the aliased method must not raise");
    assert_eq!(unsafe { got.unbox_integer() }, 7);
}

#[test]
fn include_module_mixes_in_and_rejects_a_cyclic_include() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let helper = mrb
        .define_module(c"BeniMixin")
        .expect("defining the module must succeed");
    helper
        .define_method(&mrb, c"helped", beni::method!(answer_seven, 0))
        .expect("registering the module method must succeed");

    let class = mrb
        .define_class(c"BeniHost", object)
        .expect("defining the class must succeed");
    class
        .include_module(&mrb, helper)
        .expect("including the module must succeed");

    // An instance of the host now answers the mixed-in method.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"helped", &[])
        .expect("the mixed-in method must not raise");
    assert_eq!(unsafe { got.unbox_integer() }, 7);

    // Including a module into itself is a cyclic include — rejected.
    assert!(helper.include_module(&mrb, helper).is_err());
}

#[test]
fn prepend_module_overrides_the_receiver_and_rejects_a_cyclic_prepend() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let helper = mrb
        .define_module(c"BeniPrependMixin")
        .expect("defining the module must succeed");
    // The module answers a method the host also defines, plus one only
    // it provides — to prove insert-ahead ancestry and reachability.
    helper
        .define_method(&mrb, c"answer", beni::method!(answer_nine, 0))
        .expect("registering the override must succeed");
    helper
        .define_method(&mrb, c"helped", beni::method!(answer_seven, 0))
        .expect("registering the module method must succeed");

    let class = mrb
        .define_class(c"BeniPrependHost", object)
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the host method must succeed");
    class
        .prepend_module(&mrb, helper)
        .expect("prepending the module must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // The prepended module sits ahead of the receiver, so its method wins.
    let overridden = receiver
        .funcall(&mrb, c"answer", &[])
        .expect("the overriding method must not raise");
    assert_eq!(unsafe { overridden.unbox_integer() }, 9);

    // A method only the prepended module defines is callable.
    let only = receiver
        .funcall(&mrb, c"helped", &[])
        .expect("the module-only method must not raise");
    assert_eq!(unsafe { only.unbox_integer() }, 7);

    // Prepending a module into itself is a cyclic prepend — rejected.
    assert!(helper.prepend_module(&mrb, helper).is_err());
}

#[test]
fn alias_method_surfaces_name_error_for_missing_original() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniAliasMissing", object)
        .expect("defining the class must succeed");

    // mruby raises NameError when the aliased method does not exist
    // (vendored src/class.c mrb_method_search) — it must surface as
    // Err, not a longjmp.
    let err = class
        .alias_method(&mrb, c"shadow", c"no_such_method")
        .expect_err("aliasing a missing method must surface as Err");
    assert!(matches!(err, Error::Exception(_)));
    assert!(
        err.message(&mrb).contains("no_such_method"),
        "the NameError must name the missing method: {}",
        err.message(&mrb)
    );
}

#[test]
fn undef_method_marks_a_method_undefined_on_the_handle() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniUndef", object)
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the method must succeed");

    // The method responds before undefinition.
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"answer", &[])
        .expect("the defined method must be callable before undefinition");
    assert_eq!(unsafe { got.unbox_integer() }, 7);

    class
        .undef_method(&mrb, c"answer")
        .expect("undefining a defined method must succeed");

    // After undefinition, VM dispatch raises NoMethodError — the
    // consumer-visible point of marking the name as not defined.
    let cxt = beni::Ccontext::new(&mrb, c"undef_test.rb")
        .expect("allocating the compile context must succeed");
    let err = cxt
        .load_nstring(b"BeniUndef.new.answer")
        .expect_err("dispatching an undefined method must raise");
    let message = err.message(&mrb);
    assert!(
        message.contains("answer"),
        "the NoMethodError must name the undefined method: {message}"
    );
    mrb.clear_exc();
}

#[test]
fn undef_method_surfaces_name_error_for_absent_method() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniUndefMissing", object)
        .expect("defining the class must succeed");

    // The name key routes through the raising `_id` form (the string
    // C function does not raise), so undefining a name absent from the
    // handle and its ancestors surfaces as Err, not a longjmp.
    let err = class
        .undef_method(&mrb, c"no_such_method")
        .expect_err("undefining an absent method must surface as Err");
    assert!(matches!(err, Error::Exception(_)));
    assert!(
        err.message(&mrb).contains("no_such_method"),
        "the NameError must name the absent method: {}",
        err.message(&mrb)
    );
}

#[test]
fn remove_method_strips_a_method_defined_on_the_handle() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniRemove", object)
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"answer", beni::method!(answer_seven, 0))
        .expect("registering the method must succeed");

    // The method responds before removal.
    let answer = c"answer".into_sym(&mrb);
    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    assert!(
        receiver.respond_to(&mrb, answer),
        "the defined method must respond before removal"
    );

    class
        .remove_method(&mrb, c"answer")
        .expect("removing a defined method must succeed");

    // After removal the definition is gone, so the receiver no longer
    // responds to the name.
    assert!(
        !receiver.respond_to(&mrb, answer),
        "the removed method must no longer respond"
    );
}

#[test]
fn remove_method_surfaces_name_error_for_absent_method() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniRemoveMissing", object)
        .expect("defining the class must succeed");

    // Removing a name not defined directly on the handle raises
    // NameError, caught by `protect` into Err rather than a longjmp.
    let err = class
        .remove_method(&mrb, c"no_such_method")
        .expect_err("removing an absent method must surface as Err");
    assert!(matches!(err, Error::Exception(_)));
    assert!(
        err.message(&mrb).contains("no_such_method"),
        "the NameError must name the absent method: {}",
        err.message(&mrb)
    );
}

#[test]
fn undef_singleton_method_marks_a_class_method_undefined() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    let class = mrb
        .define_class(c"BeniUndefClassMethod", object)
        .expect("defining the class must succeed");
    class
        .define_singleton_method(&mrb, c"class_answer", beni::method!(answer_nine, 0))
        .expect("registering the class method must succeed");

    // The class method responds before undefinition.
    // SAFETY: `class` is a live handle from this VM.
    let class_value = unsafe { class.to_value(&mrb) };
    let got = class_value
        .funcall(&mrb, c"class_answer", &[])
        .expect("the class method must be callable before undefinition");
    assert_eq!(unsafe { got.unbox_integer() }, 9);

    class
        .undef_singleton_method(&mrb, c"class_answer")
        .expect("undefining a defined class method must succeed");

    // After undefinition, dispatching the class method raises
    // NoMethodError — the singleton-method form of the contract.
    let cxt = beni::Ccontext::new(&mrb, c"undef_class_test.rb")
        .expect("allocating the compile context must succeed");
    cxt.load_nstring(b"BeniUndefClassMethod.class_answer")
        .expect_err("dispatching an undefined class method must raise");

    // Undefining an absent class method surfaces as Err.
    let err = class
        .undef_singleton_method(&mrb, c"never_defined")
        .expect_err("undefining an absent class method must surface as Err");
    assert!(matches!(err, Error::Exception(_)));
}

#[test]
fn real_returns_a_real_class_handle_unchanged() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A real class resolves to itself: the handle a safe lookup
    // hands back is already past any singleton / include link, so
    // `real` returns the same class — same pointer, same name.
    let object = mrb.object_class();
    let resolved = object.real();
    assert_eq!(resolved.as_raw(), object.as_raw());
    assert_eq!(resolved.name(&mrb), "Object");

    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is present in every VM");
    assert_eq!(runtime_error.real().as_raw(), runtime_error.as_raw());
    assert_eq!(runtime_error.real().name(&mrb), "RuntimeError");
}

#[test]
fn name_survives_a_gc_cycle() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // `name` owns its bytes: mruby builds the name into a GC-managed
    // temporary, so a handle held across a collection — the anonymous
    // case synthesizes a fresh `#<Class:0x..>` string each call — must
    // keep reading correctly rather than dangle into freed storage.
    let object = mrb.object_class();
    let named = object.name(&mrb);
    let anonymous = mrb
        .class_new(object)
        .expect("creating an anonymous class under Object must succeed");
    let synthesized = anonymous.name(&mrb);

    mrb.full_gc();

    assert_eq!(named, "Object");
    assert!(
        synthesized.starts_with("#<Class:"),
        "the synthesized name must survive collection: {synthesized:?}"
    );
    assert_eq!(anonymous.name(&mrb), synthesized);
}

#[test]
fn real_resolves_a_singleton_class_to_its_attached_object_class() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // A class's singleton class is an SCLASS whose real class is the
    // first user-facing class above it. The metaclass of `Object`
    // descends from `Class`, so resolving it lands on a named real
    // class rather than the `#<Class:...>` singleton form — the
    // normalization a consumer needs after reaching a singleton
    // handle through the raw seam.
    let target = mrb
        .define_class(c"BeniRealTarget", object)
        .expect("defining the class must succeed");

    // `target.singleton_class` reached through Ruby, downcast to its
    // class pointer through the raw seam, is a singleton handle.
    let cxt = beni::Ccontext::new(&mrb, c"real_sclass.rb")
        .expect("allocating the compile context must succeed");
    let sclass_val = cxt
        .load_nstring(b"BeniRealTarget.singleton_class")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "reaching the singleton class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    // A singleton class carries `MRB_TT_SCLASS`, not the plain class
    // tag `is_class` gates on, yet it still wraps an `RClass` the
    // `mrb_class_ptr` cast recovers — the raw-seam handle a consumer
    // would hold before normalizing.
    // SAFETY: `singleton_class` returns a class-family value (SCLASS),
    // so its payload is an `RClass` pointer the cast reads.
    let sclass = RClass::from_raw(unsafe { sclass_val.as_class_ptr() });

    // The singleton handle is not itself a real class — its name is
    // the `#<Class:...>` form — but `real` walks past it to a named
    // user-facing class.
    let resolved = sclass.real();
    assert_ne!(resolved.as_raw(), sclass.as_raw());
    let name = resolved.name(&mrb);
    assert!(
        !name.starts_with("#<"),
        "real must skip the singleton class, got: {name}"
    );
    let _ = target;
}

#[test]
fn exc_new_builds_an_exception_of_the_class_without_raising() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is present in every VM");

    let exc = runtime_error.exc_new(&mrb, "something failed");

    // Building does not raise; the object carries the class and the
    // message verbatim, ready to ride out as Error::Exception.
    assert!(mrb.pending_exc().is_nil(), "exc_new must not raise");
    assert_eq!(exc.classname(&mrb), "RuntimeError");
    assert_eq!(Error::Exception(exc).message(&mrb), "something failed");
}

#[test]
fn exc_new_str_carries_an_existing_string_value_without_raising() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let runtime_error = mrb
        .class_get(c"RuntimeError")
        .expect("RuntimeError is present in every VM");

    // A message the consumer already holds as an RString rides into
    // the exception as-is — the String-valued counterpart of exc_new.
    let message = mrb.str_new(b"already a string");
    let exc = runtime_error.exc_new_str(&mrb, message);

    assert!(mrb.pending_exc().is_nil(), "exc_new_str must not raise");
    assert_eq!(exc.classname(&mrb), "RuntimeError");
    assert_eq!(Error::Exception(exc).message(&mrb), "already a string");
}

#[test]
fn path_reads_the_qualified_namespace_chain() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // A nested class reads its full namespace path, not just its leaf
    // name — the consumer-visible point of the path read.
    let outer = mrb
        .define_module(c"BeniPathNs")
        .expect("defining the outer module must succeed");
    let inner = outer
        .define_class(&mrb, c"Inner", object)
        .expect("defining the nested class must succeed");
    assert_eq!(inner.path(&mrb), Some("BeniPathNs::Inner".to_string()));

    // A top-level handle's path is its bare name.
    let top = mrb
        .define_class(c"BeniPathTop", object)
        .expect("defining the top-level class must succeed");
    assert_eq!(top.path(&mrb), Some("BeniPathTop".to_string()));

    // The read is total — it leaves no pending exception behind.
    assert!(mrb.pending_exc().is_nil(), "path must not raise");
}

#[test]
fn path_yields_none_for_an_anonymous_class() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let object = mrb.object_class();

    // An anonymous class has no place in any namespace, so its path is
    // nothing — distinct from `name`, which synthesizes a stand-in.
    let anon = mrb
        .class_new(object)
        .expect("creating an anonymous class under Object must succeed");
    assert_eq!(anon.path(&mrb), None);
    assert!(mrb.pending_exc().is_nil(), "path must not raise");
}
