use crate::support::open_mrb;
use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::{Array, Error, FromValue, Hash, IntoValue, Mrb, Proc, Value};

/// Define `name` on `Object` as a `-1` method running `body`.
fn define(mrb: &Mrb, name: &core::ffi::CStr, def: beni::MethodDef) {
    mrb.object_class()
        .define_method(mrb, name, def)
        .expect("registering the reader must succeed");
}

/// Run `source` and read its result as `T`, failing on any raise.
fn eval<T: FromValue>(mrb: &Mrb, source: &str) -> T {
    let value = mrb
        .load_string(source.as_bytes())
        .unwrap_or_else(|err| panic!("{source} raised: {}", err.message(mrb)));
    T::from_value(value).unwrap_or_else(|| panic!("{source} answered {}", value.inspect(mrb)))
}

/// Run `source`, which must raise, and answer the exception's class name
/// and message.
fn raise_of(mrb: &Mrb, source: &str) -> (String, String) {
    match mrb.load_string(source.as_bytes()) {
        Err(Error::Exception(exc)) => (exc.classname(mrb), Error::Exception(exc).message(mrb)),
        other => panic!("{source} must raise, got {other:?}"),
    }
}

fn ints(mrb: &Mrb, values: &[i32]) -> Value {
    let values: Vec<Value> = values.iter().map(|v| v.into_value(mrb)).collect();
    mrb.ary_new_from_values(&values).as_value()
}

// def m(a, b = nil, *rest, c) — answered as [a, b || -1, *rest, c].
fn every_positional_part(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args = scan_args::<(i32,), (Option<i32>,), Vec<i32>, (i32,), (), ()>(mrb)?;
    let (a,) = args.required;
    let (b,) = args.optional;
    let (c,) = args.trailing;
    let mut out = vec![a, b.unwrap_or(-1)];
    out.extend(args.splat);
    out.push(c);
    Ok(ints(mrb, &out))
}

#[test]
fn positionals_fill_required_and_trailing_before_optional_and_splat() {
    let mrb = open_mrb();
    define(&mrb, c"parts", beni::method!(every_positional_part, -1));

    let read = |src| eval::<Array>(&mrb, src).as_value().inspect(&mrb);
    assert_eq!(read("parts(1, 2)"), "[1, -1, 2]");
    assert_eq!(read("parts(1, 2, 3)"), "[1, 2, 3]");
    assert_eq!(read("parts(1, 2, 3, 4, 5)"), "[1, 2, 3, 4, 5]");
}

// def m(a, b = nil)
fn bounded(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    scan_args::<(Value,), (Option<Value>,), (), (), (), ()>(mrb)?;
    Ok(Value::nil())
}

#[test]
fn a_positional_count_outside_the_shape_is_mrubys_argument_error() {
    let mrb = open_mrb();
    define(&mrb, c"parts", beni::method!(every_positional_part, -1));
    define(&mrb, c"bounded", beni::method!(bounded, -1));

    assert_eq!(
        raise_of(&mrb, "parts(1)"),
        (
            "ArgumentError".into(),
            "wrong number of arguments (given 1, expected 2+)".into()
        )
    );
    assert_eq!(
        raise_of(&mrb, "bounded(1, 2, 3)"),
        (
            "ArgumentError".into(),
            "wrong number of arguments (given 3, expected 1..2)".into()
        )
    );
}

#[test]
fn a_positional_of_the_wrong_type_is_a_type_error() {
    let mrb = open_mrb();
    define(&mrb, c"parts", beni::method!(every_positional_part, -1));

    assert_eq!(raise_of(&mrb, "parts('x', 2)").0, "TypeError");
    assert_eq!(raise_of(&mrb, "parts(1, 2, 'x', 4)").0, "TypeError");
}

// def m(*rest, **kw) — answered as [rest.size, kw].
fn keyword_bucket(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args = scan_args::<(), (), Array, (), Hash, ()>(mrb)?;
    Ok(mrb
        .ary_new_from_values(&[
            (args.splat.len() as i32).into_value(mrb),
            args.keywords.as_value(),
        ])
        .as_value())
}

#[test]
fn the_keyword_bucket_holds_keywords_apart_from_the_positionals() {
    let mrb = open_mrb();
    define(&mrb, c"kw", beni::method!(keyword_bucket, -1));

    let read = |src| eval::<Array>(&mrb, src).as_value().inspect(&mrb);
    assert_eq!(read("kw(1, a: 2)"), "[1, {a: 2}]");
    assert_eq!(read("kw(1, {a: 2})"), "[2, {}]");
    assert_eq!(read("kw"), "[0, {}]");
}

// def m(*rest) — answered as the splat itself.
fn splat_only(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Ok(scan_args::<(), (), Array, (), (), Option<Proc>>(mrb)?
        .splat
        .as_value())
}

#[test]
fn without_a_keyword_part_the_keywords_read_as_the_last_positional() {
    let mrb = open_mrb();
    define(&mrb, c"splat", beni::method!(splat_only, -1));

    assert_eq!(
        eval::<Array>(&mrb, "splat(1, a: 2)")
            .as_value()
            .inspect(&mrb),
        "[1, {a: 2}]"
    );
}

#[test]
fn an_array_splat_and_an_optional_block_fit_every_call() {
    let mrb = open_mrb();
    define(&mrb, c"splat", beni::method!(splat_only, -1));

    for src in [
        "splat",
        "splat(1, 2, 3)",
        "splat(a: 1) { }",
        "splat(*(1..20), k: 1)",
    ] {
        eval::<Array>(&mrb, src);
    }
}

// Holds the splat across a full collection and a dispatch before reading it.
fn splat_across_reentry(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let splat = scan_args::<(), (), Array, (), (), ()>(mrb)?.splat;
    mrb.full_gc();
    splat.as_value().funcall(mrb, "inspect", &[])?;
    Ok(splat.as_value())
}

#[test]
fn an_array_splat_survives_a_collection_and_a_reentry() {
    let mrb = open_mrb();
    define(&mrb, c"held", beni::method!(splat_across_reentry, -1));

    assert_eq!(
        eval::<Array>(&mrb, "held('a' * 3, 'b' * 3)")
            .as_value()
            .inspect(&mrb),
        r#"["aaa", "bbb"]"#
    );
}

fn required_block(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let block = scan_args::<(), (), (), (), (), Proc>(mrb)?.block;
    block.call(mrb, &[])
}

fn optional_block(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Ok(scan_args::<(), (), (), (), (), Option<Proc>>(mrb)?
        .block
        .is_some()
        .into_value(mrb))
}

fn ignored_block(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    scan_args::<(), (), (), (), (), ()>(mrb)?;
    Ok(Value::nil())
}

#[test]
fn the_block_part_requires_accepts_or_ignores_the_block() {
    let mrb = open_mrb();
    define(&mrb, c"required", beni::method!(required_block, -1));
    define(&mrb, c"optional", beni::method!(optional_block, -1));
    define(&mrb, c"ignored", beni::method!(ignored_block, -1));

    assert_eq!(eval::<i32>(&mrb, "required { 7 }"), 7);
    assert_eq!(
        raise_of(&mrb, "required"),
        ("ArgumentError".into(), "no block given".into())
    );
    assert!(eval::<bool>(&mrb, "optional { }"));
    assert!(!eval::<bool>(&mrb, "optional"));
    assert!(eval::<Value>(&mrb, "ignored { }").is_nil());
}

thread_local! {
    static GUARD_DROPS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

struct ReadGuard;

impl Drop for ReadGuard {
    fn drop(&mut self) {
        GUARD_DROPS.with(|drops| drops.set(drops.get() + 1));
    }
}

fn guarded(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let _guard = ReadGuard;
    scan_args::<(i32,), (), (), (), (), Proc>(mrb)?;
    Ok(Value::nil())
}

#[test]
fn a_call_that_does_not_fit_leaves_the_body_through_its_own_return() {
    let mrb = open_mrb();
    define(&mrb, c"guarded", beni::method!(guarded, -1));

    for src in ["guarded", "guarded('x') { }", "guarded(1)"] {
        let before = GUARD_DROPS.with(core::cell::Cell::get);
        raise_of(&mrb, src);
        assert_eq!(GUARD_DROPS.with(core::cell::Cell::get), before + 1, "{src}");
    }
}
