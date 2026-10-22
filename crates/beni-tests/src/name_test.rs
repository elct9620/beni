use crate::support::{open_mrb, OwnedBytes};
use beni::prelude::*;
use beni::{
    ConstId, CvarId, Error, FromValue, GvarId, Id, IntoValue, IvarId, Mrb, RObject, Symbol, Value,
};

fn object(mrb: &Mrb) -> RObject {
    RObject::from_value(
        mrb.eval::<beni::Value>("Object.new")
            .expect("Object.new runs"),
    )
    .expect("Object.new answers an object")
}

fn id(mrb: &Mrb, name: &str) -> Id {
    mrb.intern(name.as_bytes()).expect("a short name interns")
}

/// The refusal an operation surfaces: the `NameError`'s message and the
/// name it carries.
fn refusal(mrb: &Mrb, err: Error) -> (String, Option<Id>) {
    let Error::Exception(exc) = err else {
        panic!("a refused name must surface as an exception, got {err}");
    };
    assert_eq!(exc.classname(mrb), "NameError");
    let name = exc
        .funcall(mrb, c"name", ())
        .expect("NameError#name answers");
    let message = Error::Exception(exc).message(mrb);
    (message, Symbol::from_value(name).map(Id::from))
}

#[test]
fn an_instance_variable_name_no_ruby_program_can_write_is_refused_naming_it() {
    let mrb = open_mrb();
    let holder = object(&mrb);

    let err = holder
        .ivar_set(&mrb, c"__secret__", 1)
        .expect_err("a name without `@` is refused");

    assert_eq!(
        refusal(&mrb, err),
        (
            "'__secret__' is not allowed as an instance variable name".to_owned(),
            Some(id(&mrb, "__secret__")),
        )
    );
    assert!(holder.ivar_get::<_, Value>(&mrb, c"__secret__").is_err());
    assert!(!holder.ivar_defined(&mrb, c"__secret__"));
    assert!(holder.ivar_remove(&mrb, c"__secret__").is_err());
    assert!(holder.ivar_set(&mrb, id(&mrb, "@1"), 1).is_err());
    assert!(holder.ivar_set(&mrb, id(&mrb, "@open"), 1).is_ok());
}

#[test]
fn the_iteration_skips_a_hidden_name_on_a_plain_object() {
    let mrb = open_mrb();
    let holder = object(&mrb);
    holder
        .ivar_set(&mrb, c"@open", 1)
        .expect("an `@` name assigns");
    let hidden = id(&mrb, "__hidden__");
    // SAFETY: `mrb` is live and `holder` belongs to it; a hidden name on
    // a plain object is what `beni::sys` alone reaches.
    unsafe {
        beni::sys::mrb_iv_set(
            mrb.as_ptr(),
            beni::sys::AsRawValue::as_raw(holder.as_value()),
            beni::sys::AsRawId::as_raw(hidden),
            beni::sys::AsRawValue::as_raw(2i32.into_value(&mrb)),
        );
    }

    let mut visited = Vec::new();
    holder.ivar_foreach(&mrb, |name, _| {
        visited.push(Id::from(name));
        beni::ForEach::Continue
    });

    assert_eq!(visited, [id(&mrb, "@open")]);
}

#[test]
fn a_singleton_class_keeps_what_it_is_attached_to() {
    let mrb = open_mrb();
    let holder = object(&mrb);
    let singleton = holder.singleton_class(&mrb).expect("an object has one");

    assert!(singleton.ivar_set(&mrb, c"__attached__", 1).is_err());
    assert!(singleton.const_set(&mrb, c"__attached__", 1).is_err());
    assert!(singleton.cvar_set(&mrb, c"__attached__", 1).is_err());
    mrb.gv_set(c"$holder", holder.as_value())
        .expect("assigning a global succeeds");
    let missing = mrb.eval::<beni::Value>("begin; (class << $holder; self; end).class_variable_get(:@@none); rescue NameError; :missing; end");

    assert!(Symbol::from_value(missing.expect("the lookup runs")).is_some());
}

#[test]
fn class_variable_and_constant_names_follow_ruby() {
    let mrb = open_mrb();
    let class = mrb.class_object();

    let cvar = class
        .cvar_set(&mrb, c"@one", 1)
        .expect_err("a class variable needs `@@`");
    let constant = class
        .const_set(&mrb, c"lower", 1)
        .expect_err("a constant starts uppercase");

    assert_eq!(
        refusal(&mrb, cvar).0,
        "'@one' is not allowed as a class variable name"
    );
    assert_eq!(refusal(&mrb, constant).0, "wrong constant name lower");
    assert!(mrb.define_class(c"lower", class).is_err());
    assert!(mrb.define_module(c"__hidden__").is_err());
    assert!(!class.const_defined(&mrb, c"lower"));
    assert!(!mrb.class_defined(c"__hidden__"));
    assert!(class.cvar_set(&mrb, c"@@one", 1).is_ok());
    assert!(class.const_set(&mrb, c"BeniNamed", 1).is_ok());
}

#[test]
fn a_global_without_dollar_is_the_dollar_global() {
    let mrb = open_mrb();

    mrb.gv_set(c"beni_named", 7i32.into_value(&mrb))
        .expect("assigning a global succeeds");
    mrb.gv_remove(c"_gc_root_");
    let read = mrb
        .eval::<beni::Value>("$beni_named")
        .expect("reading a global runs");

    assert_eq!(i32::from_value(read), Some(7));
    assert_eq!(
        i32::from_value(
            mrb.gv_get::<_, Value>(id(&mrb, "beni_named"))
                .expect("the global reads")
        ),
        Some(7)
    );
}

#[test]
fn the_lazy_record_survives_its_name_given_to_a_global_operation() {
    let mrb = open_mrb();
    // Rooted inside a scope, so only the record keeps the string and the
    // record's own global keeps the record.
    let scope = mrb.arena_scope();
    let root = mrb
        .gc_root(mrb.str_new(b"rooted").as_value())
        .expect("rooting succeeds");
    drop(scope);

    mrb.gv_set(c"beni_lazy", 1i32.into_value(&mrb))
        .expect("assigning a global succeeds");
    mrb.gv_remove(c"beni_lazy");
    mrb.gc_start();

    assert_eq!(
        beni::RString::from_value(root.value())
            .expect("the rooted value is still a String")
            .owned_bytes(),
        b"rooted"
    );
}

#[test]
fn a_checked_key_keys_its_namespace_and_converts_into_its_id() {
    let mrb = open_mrb();
    let class = mrb.class_object();
    let constant = ConstId::new(&mrb, c"BeniChecked").expect("an uppercase name checks");
    let ivar = IvarId::new(&mrb, c"@checked").expect("an `@` name checks");
    let holder = object(&mrb);
    holder
        .ivar_set(&mrb, ivar, 2)
        .expect("the checked key assigns");
    assert_eq!(
        i32::from_value(holder.ivar_get(&mrb, ivar).expect("it reads back")),
        Some(2)
    );
    assert!(IvarId::new(&mrb, c"checked").is_err());

    class
        .const_set(&mrb, constant, 1)
        .expect("the checked key assigns");

    assert_eq!(
        i32::from_value(class.const_get(&mrb, constant).expect("it reads back")),
        Some(1)
    );
    assert_eq!(Id::from(constant), id(&mrb, "BeniChecked"));
    assert!(ConstId::new(&mrb, c"checked").is_err());
    let cvar = CvarId::new(&mrb, c"@@checked").expect("an `@@` name checks");
    assert!(class.const_set(&mrb, cvar, 1).is_err());
    assert_eq!(
        Id::from(GvarId::new(&mrb, c"checked").expect("a global name resolves")),
        id(&mrb, "$checked")
    );
}

#[test]
fn a_name_too_long_to_intern_is_refused_as_too_long() {
    let mrb = open_mrb();
    let long = "x".repeat(usize::from(u16::MAX));

    let err = mrb
        .class_object()
        .const_set(&mrb, long.as_str(), 1)
        .expect_err("a too-long name is refused");

    let Error::Exception(exc) = err else {
        panic!("the refusal must be an exception");
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
}
