use crate::support::{open_mrb, same_object, Is, OwnedBytes};
use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::{
    Ccontext, Error, FromValue, IntoValue, Module, Mrb, Proc, RArray, RClass, RModule, RObject,
    Symbol, Value,
};

/// Yielder method in the boundary-terminating shape kobako uses:
/// read the captured (non-orphan) block, yield it, and on a real
/// `break` report its carried value back as the method's result.
fn report_break(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let block = scan_args::<(Symbol,), (), RArray, (), (), Proc>(mrb)?.block;
    Ok(match block.call(mrb, &[]) {
        Ok(_) => (-1i32).into_value(mrb),
        Err(Error::Exception(exc)) => match exc.as_break() {
            Some(brk) => brk.value(),
            None => (-2i32).into_value(mrb),
        },
        Err(_) => (-3i32).into_value(mrb),
    })
}

#[test]
fn as_break_rejects_non_break_values() {
    let mrb = open_mrb();

    // No ordinary value carries the break tag — including a real
    // exception object (a raise is not a break).
    assert!(42i32.into_value(&mrb).as_break().is_none());
    assert!(mrb.str_new(b"x").as_value().as_break().is_none());
    assert!(Value::nil().as_break().is_none());
}

#[test]
fn funcall_dispatches_a_method_and_returns_its_value() {
    let mrb = open_mrb();

    // `42.to_s` dispatches `Integer#to_s` and hands back its String.
    let got = 42i32
        .into_value(&mrb)
        .funcall(&mrb, c"to_s", &[])
        .expect("a non-raising dispatch must come back Ok");
    assert_eq!(got.to_string(&mrb), "42");
}

#[test]
fn funcall_passes_the_argument_slice_to_the_method() {
    let mrb = open_mrb();

    // `40 + 2` proves the arg slice reaches the dispatched method.
    let got = 40i32
        .into_value(&mrb)
        .funcall(&mrb, c"+", &[2i32.into_value(&mrb)])
        .expect("a non-raising dispatch must come back Ok");
    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn funcall_surfaces_a_raised_exception_as_err() {
    let mrb = open_mrb();

    // Dispatching an undefined method raises `NoMethodError`, which the
    // protect frame catches into `Err` rather than long-jumping.
    let err = 42i32
        .into_value(&mrb)
        .funcall(&mrb, c"no_such_method", &[])
        .expect_err("a raising dispatch must surface as Err");
    match err {
        Error::Exception(_) => {}
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }
    // The VM stays usable after the protected raise.
    let again = 7i32
        .into_value(&mrb)
        .funcall(&mrb, c"to_s", &[])
        .expect("the VM must survive the protected raise");
    assert_eq!(again.to_string(&mrb), "7");
}

#[test]
fn funcall_accepts_a_symbol_key_identical_to_the_name() {
    let mrb = open_mrb();

    // An interned `Symbol` key reaches the same dispatch as the
    // equivalent name, proving the `IntoId` generalization routes
    // both through the same interned id.
    let recv = 42i32.into_value(&mrb);
    let by_name = recv
        .funcall(&mrb, c"to_s", &[])
        .expect("the name key must dispatch");
    let by_sym = recv
        .funcall(
            &mrb,
            Symbol::new(&mrb, c"to_s").expect("the name interns"),
            &[],
        )
        .expect("the symbol key must dispatch");
    assert_eq!(by_name.to_string(&mrb), by_sym.to_string(&mrb));
    assert_eq!(by_sym.to_string(&mrb), "42");
}

#[test]
fn funcall_with_block_yields_to_the_passed_block() {
    let mrb = open_mrb();

    // A block that records each doubled element into a global array.
    // `Array#each` yields every element to it; reading `$seen` back
    // proves the block reached the dispatched method and ran.
    let cxt = Ccontext::new(&mrb, c"funcall_block.rb")
        .expect("allocating the compile context must succeed");
    cxt.load_nstring(b"$seen = []")
        .expect("the test source must compile and run");
    let block = Proc::from_value(
        cxt.load_nstring(b"proc { |x| $seen << x * 2 }")
            .expect("the test source must compile and run"),
    )
    .expect("a proc literal carries MRB_TT_PROC");

    let receiver = cxt
        .load_nstring(b"[1, 2, 3]")
        .expect("the test source must compile and run");
    receiver
        .funcall_with_block(&mrb, c"each", &[], block)
        .expect("yielding through each must come back Ok");

    assert_eq!(
        cxt.load_nstring(b"$seen")
            .expect("the test source must compile and run")
            .to_string(&mrb),
        "[2, 4, 6]"
    );
}

#[test]
fn funcall_with_block_surfaces_a_raised_exception_as_err() {
    let mrb = open_mrb();

    let cxt = Ccontext::new(&mrb, c"funcall_block_raise.rb")
        .expect("allocating the compile context must succeed");
    let block = Proc::from_value(
        cxt.load_nstring(b"proc { raise 'boom from block' }")
            .expect("the test source must compile and run"),
    )
    .expect("a proc literal carries MRB_TT_PROC");

    // The block raises while `each` yields to it; the protect frame
    // catches the raise into `Err` rather than long-jumping across FFI.
    let receiver = cxt
        .load_nstring(b"[1]")
        .expect("the test source must compile and run");
    let err = receiver
        .funcall_with_block(&mrb, c"each", &[], block)
        .expect_err("a raising block must surface as Err");
    match err {
        Error::Exception(_) => assert!(err.message(&mrb).contains("boom from block")),
        other => panic!("a Ruby raise must surface as Error::Exception, got {other}"),
    }

    // The VM stays usable after the protected raise.
    let again = 7i32
        .into_value(&mrb)
        .funcall(&mrb, c"to_s", &[])
        .expect("the VM must survive the protected raise");
    assert_eq!(again.to_string(&mrb), "7");
}

#[test]
fn the_string_downcast_discriminates_the_string_tag() {
    let mrb = open_mrb();

    assert!(mrb.str_new(b"x").as_value().is::<beni::RString>());
    // A non-String tag — and an immediate — both reject.
    assert!(!42i32.into_value(&mrb).is::<beni::RString>());
    assert!(!Value::nil().is::<beni::RString>());
}

#[test]
fn handle_downcasts_discriminate_module_range_and_exception() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"tag_pred_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"Enumerable")
        .expect("the test source must compile and run");
    let range = cxt
        .load_nstring(b"(1..3)")
        .expect("the test source must compile and run");
    let exception = cxt
        .load_nstring(b"RuntimeError.new('boom')")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "the literals must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // Each handle accepts exactly its own tag.
    assert!(module.is::<beni::RModule>());
    assert!(range.is::<beni::Range>());
    assert!(exception.is::<beni::Exception>());

    // A class is not a module: the class and module handles split the
    // class-family tags, and neither claims the other's value.
    let class = cxt
        .load_nstring(b"String")
        .expect("the test source must compile and run");
    assert!(class.is::<beni::RClass>());
    assert!(!class.is::<beni::RModule>());
    assert!(!module.is::<beni::RClass>());

    // A singleton class converts into a class handle, never a module's.
    let singleton = cxt
        .load_nstring(b"'beni'.singleton_class")
        .expect("the test source must compile and run");
    assert!(singleton.is::<beni::RClass>());
    assert!(!singleton.is::<beni::RModule>());

    // No handle claims an unrelated tag, nor an immediate.
    assert!(!range.is::<beni::RModule>());
    assert!(!exception.is::<beni::Range>());
    assert!(!42i32.into_value(&mrb).is::<beni::Exception>());
    assert!(!Value::nil().is::<beni::RModule>());
}

#[test]
fn to_string_reads_a_string_subclass_result() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"to_s_test.rb").expect("allocating the context must succeed");

    // `to_s` returns a String *subclass* instance: String-tagged, so it
    // reads the same way as a plain String. The tag, not the classname,
    // decides — the subclass result converts rather than collapsing to
    // an empty string.
    let obj = cxt.load_nstring(
        b"class BeniSubStr < String; end; class BeniHasSubToS; def to_s; BeniSubStr.new('sub'); end; end; BeniHasSubToS.new",
    ).expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the classes must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    assert_eq!(obj.to_string(&mrb), "sub");
}

#[test]
fn inspect_renders_the_ruby_debug_string() {
    let mrb = open_mrb();

    // A String inspects quoted, an Integer and nil render canonically —
    // the debug forms, not the to_s forms.
    assert_eq!(mrb.str_new(b"hi").as_value().inspect(&mrb), "\"hi\"");
    assert_eq!(42i32.into_value(&mrb).inspect(&mrb), "42");
    assert_eq!(Value::nil().inspect(&mrb), "nil");
}

#[test]
fn inspect_swallows_a_raising_inspect_as_empty() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"inspect_test.rb")
        .expect("allocating the compile context must succeed");

    let obj = cxt
        .load_nstring(b"class BoomInspect; def inspect; raise 'no'; end; end; BoomInspect.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // A raising user inspect is swallowed into an empty string, and the
    // protect frame leaves no pending exception behind.
    assert_eq!(obj.inspect(&mrb), String::new());
    assert!(
        mrb.pending_exc().is_nil(),
        "the swallowed raise must not leave a pending exception"
    );
}

#[test]
fn any_to_s_renders_the_default_object_form() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"any_to_s_test.rb")
        .expect("allocating the compile context must succeed");

    // A user class with no to_s override renders the default
    // `#<ClassName:0x...>` form, built from the class name without
    // dispatching the receiver's own to_s.
    let obj = cxt
        .load_nstring(b"class Plain; end; Plain.new")
        .expect("the test source must compile and run");
    let rendered = obj.any_to_s(&mrb).owned_bytes();
    assert!(
        rendered.starts_with(b"#<Plain:0x"),
        "expected the default heap-object form, got {:?}",
        String::from_utf8_lossy(&rendered)
    );
    assert!(rendered.ends_with(b">"));
}

#[test]
fn equality_separates_value_eql_and_identity() {
    let mrb = open_mrb();

    let a = mrb.str_new(b"hello").as_value();
    let b = mrb.str_new(b"hello").as_value();
    let c = mrb.str_new(b"world").as_value();

    // `==` and `eql?` are by value: distinct String objects with the
    // same content compare equal, differing content does not.
    assert!(a.equal(&mrb, b).expect("== does not raise for strings"));
    assert!(a.eql(&mrb, b).expect("eql? does not raise for strings"));
    assert!(!a.equal(&mrb, c).expect("== does not raise for strings"));

    // `equal?` is identity: a value is the same object as itself but
    // not as a distinct equal-valued object.
    assert!(a.is_equal(&mrb, a));
    assert!(!a.is_equal(&mrb, b));
}

#[test]
fn object_id_is_stable_per_value_and_distinct_across_identity() {
    let mrb = open_mrb();

    let a = mrb.str_new(b"hello").as_value();
    let b = mrb.str_new(b"hello").as_value();

    // The id reads from identity, not content: a value's id equals
    // its own, two identity-distinct objects of equal content differ.
    assert_eq!(a.object_id(), a.object_id());
    assert_ne!(a.object_id(), b.object_id());

    // An immediate's id is likewise its own and stable.
    let n = 7i32.into_value(&mrb);
    assert_eq!(n.object_id(), 7i32.into_value(&mrb).object_id());
}

#[test]
fn equal_surfaces_a_raising_user_method_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"eq_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"class Boom; def ==(o); raise 'no'; end; end; Boom.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // Comparing dispatches the user `==`, which raises — the raise
    // surfaces as Err instead of unwinding across the call.
    let other = mrb.str_new(b"x").as_value();
    assert!(matches!(obj.equal(&mrb, other), Err(Error::Exception(_))));
}

#[test]
fn eql_surfaces_a_raising_user_method_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"eql_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"class BoomEql; def eql?(o); raise 'no'; end; end; BoomEql.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // `eql?` short-circuits on identity: comparing the object to
    // itself returns true without reaching the raising eql?.
    assert!(matches!(obj.eql(&mrb, obj), Ok(true)));

    // Distinct objects reach the dispatch — which raises, surfacing
    // as Err rather than unwinding across the call, the eql?
    // counterpart to the `==` path above.
    let other = mrb.str_new(b"x").as_value();
    assert!(matches!(obj.eql(&mrb, other), Err(Error::Exception(_))));
}

#[test]
fn cmp_ranks_comparable_values_and_yields_none_for_incomparable() {
    use core::cmp::Ordering;

    let mrb = open_mrb();

    let one = 1i32.into_value(&mrb);
    let two = 2i32.into_value(&mrb);

    // `<=>` ranks the three orderings.
    assert!(matches!(one.cmp(&mrb, two), Ok(Some(Ordering::Less))));
    assert!(matches!(one.cmp(&mrb, one), Ok(Some(Ordering::Equal))));
    assert!(matches!(two.cmp(&mrb, one), Ok(Some(Ordering::Greater))));

    // Values with no ordering between them — an integer against a
    // string — yield nothing rather than an error.
    let s = mrb.str_new(b"x").as_value();
    assert!(matches!(one.cmp(&mrb, s), Ok(None)));
}

#[test]
fn cmp_ranks_a_custom_spaceship_by_sign_not_magnitude() {
    use core::cmp::Ordering;

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"cmp_test.rb").expect("allocating the context must succeed");

    // A `<=>` is only obliged to return negative / zero / positive, so
    // a custom one can answer with any magnitude. `Wide#<=>` reports
    // "greater" as 2 and "less" as -3 to prove ranking keys on sign.
    let greater = cxt
        .load_nstring(b"class Wide; def <=>(o); 2; end; end; Wide.new")
        .expect("the test source must compile and run");
    let less = cxt
        .load_nstring(b"class Narrow; def <=>(o); -3; end; end; Narrow.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the classes must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    let other = mrb.str_new(b"x").as_value();
    assert!(matches!(
        greater.cmp(&mrb, other),
        Ok(Some(Ordering::Greater))
    ));
    assert!(matches!(less.cmp(&mrb, other), Ok(Some(Ordering::Less))));
}

#[test]
fn cmp_surfaces_a_raising_user_method_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"cmp_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"class BoomCmp; def <=>(o); raise 'no'; end; end; BoomCmp.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // Comparing dispatches the user `<=>`, which raises — the raise
    // surfaces as Err rather than unwinding across the call.
    let other = mrb.str_new(b"x").as_value();
    assert!(matches!(obj.cmp(&mrb, other), Err(Error::Exception(_))));
}

#[test]
fn dup_and_clone_surface_a_raising_initialize_copy_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"copy_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"class BoomCopy; def initialize_copy(o); raise 'no'; end; end; BoomCopy.new")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "defining the class must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    // Both copies run `initialize_copy`, which raises — surfaced as
    // Err instead of unwinding across the call.
    assert!(matches!(obj.dup(&mrb), Err(Error::Exception(_))));
    assert!(matches!(
        obj.funcall(&mrb, c"clone", &[]),
        Err(Error::Exception(_))
    ));
}

#[test]
fn check_frozen_guards_frozen_and_immediate_receivers() {
    let mrb = open_mrb();

    // A mutable heap object passes the guard.
    let mutable = mrb.str_new(b"open").as_value();
    assert!(matches!(mutable.check_frozen(&mrb), Ok(())));

    // Freezing it flips the guard to a `FrozenError`, surfaced as Err
    // rather than unwinding across the call. The protect frame leaves
    // no pending exception behind.
    let frozen = mutable.freeze(&mrb);
    let err = frozen
        .check_frozen(&mrb)
        .expect_err("a frozen receiver must surface as Err");
    match err {
        Error::Exception(exc) => {
            assert_eq!(exc.classname(&mrb), "FrozenError");
        }
        other => unreachable!("the guard must surface as Error::Exception, got {other}"),
    }
    assert!(
        mrb.pending_exc().is_nil(),
        "the caught raise must not leave a pending exception"
    );

    // An immediate counts as frozen.
    assert!(matches!(
        42i32.into_value(&mrb).check_frozen(&mrb),
        Err(Error::Exception(_))
    ));
}

#[test]
fn to_r_string_coerces_through_to_s() {
    let mrb = open_mrb();

    // Already a string: coercion returns that same string, not a copy.
    let already = mrb.str_new(b"hi").as_value();
    let coerced = already
        .to_r_string(&mrb)
        .expect("a string coerces without raising");
    assert!(coerced.is::<beni::RString>());
    assert!(already.is_equal(&mrb, coerced));

    // A non-string coerces through its `to_s`.
    assert!(42i32
        .into_value(&mrb)
        .to_r_string(&mrb)
        .expect("to_s of an integer does not raise")
        .is::<beni::RString>());
}

/// A `to_a` that returns a non-array non-`nil` value — the case that
/// makes `mrb_ary_splat` raise rather than wrap.
fn to_a_returns_int(_mrb: &Mrb, _self: Value) -> i32 {
    42
}

/// A `to_a` that returns `nil` — the case `mrb_ary_splat` wraps in a
/// one-element array holding the receiver.
fn to_a_returns_nil(_mrb: &Mrb, _self: Value) -> Value {
    Value::nil()
}

#[test]
fn to_ary_spreads_or_wraps_each_value_kind() {
    let mrb = open_mrb();

    // An array spreads to a copy: same elements, distinct object.
    let src = mrb.ary_new_from_values(&[1i32.into_value(&mrb), 2i32.into_value(&mrb)]);
    let spread = RArray::to_ary(src.as_value(), &mrb).expect("an array spreads without raising");
    assert_eq!(spread.len(), 2);
    assert!(!src.as_value().is_equal(&mrb, spread.as_value()));

    // A scalar that does not respond to `to_a` wraps in `[scalar]`.
    let wrapped =
        RArray::to_ary(7i32.into_value(&mrb), &mrb).expect("a scalar wraps without raising");
    assert_eq!(wrapped.len(), 1);
    assert_eq!(i32::from_value(wrapped.entry(&mrb, 0)), Some(7));

    // `nil` answers `to_a` with an empty array here (mruby-object-ext
    // defines `NilClass#to_a`), so it spreads to `[]` — the responder
    // path, not a wrap.
    let nil_spread = RArray::to_ary(Value::nil(), &mrb).expect("nil spreads through its to_a");
    assert_eq!(nil_spread.len(), 0);

    // A `to_a` responder whose result is an array passes that array
    // through: a Range yields its enumerated elements.
    let range = mrb
        .range_new(1i32.into_value(&mrb), 3i32.into_value(&mrb), false)
        .expect("a Range over comparable bounds constructs");
    let enumerated =
        RArray::to_ary(range.as_value(), &mrb).expect("a Range spreads through its to_a");
    assert_eq!(enumerated.len(), 3);

    // A `to_a` that returns `nil` falls back to wrapping the receiver
    // in a one-element array.
    let nil_class = mrb
        .class_new(mrb.object_class())
        .expect("an anonymous class under Object constructs");
    nil_class
        .define_method(&mrb, c"to_a", beni::method!(to_a_returns_nil, 0))
        .expect("registering to_a must succeed");
    let nil_obj = nil_class
        .new_instance(&mrb, &[])
        .expect("the class whose to_a returns nil instantiates");
    let nil_returned =
        RArray::to_ary(nil_obj, &mrb).expect("a nil-returning to_a wraps without raising");
    assert_eq!(nil_returned.len(), 1);
    assert!(nil_obj.is_equal(&mrb, nil_returned.entry(&mrb, 0)));

    // A `to_a` that returns a non-array non-`nil` value raises a
    // genuine `TypeError`, caught into the `Err` rather than wrapping.
    let class = mrb
        .class_new(mrb.object_class())
        .expect("an anonymous class under Object constructs");
    class
        .define_method(&mrb, c"to_a", beni::method!(to_a_returns_int, 0))
        .expect("registering to_a must succeed");
    let obj = class
        .new_instance(&mrb, &[])
        .expect("the class with a misbehaving to_a instantiates");
    match RArray::to_ary(obj, &mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-array non-nil to_a surfaces a TypeError Err"),
    }
}

#[test]
fn to_bool_follows_ruby_truthiness() {
    let mrb = open_mrb();

    // Only `nil` and `false` are falsy; every other value — zero
    // and the empty string included — is truthy.
    assert!(Value::true_().to_bool());
    assert!(!Value::false_().to_bool());
    assert!(!Value::nil().to_bool());
    assert!(0i32.into_value(&mrb).to_bool());
    assert!(mrb.str_new(b"").as_value().to_bool());
}

#[test]
fn dup_copies_state_into_an_independent_object() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"dup_test.rb").expect("allocating the compile context must succeed");

    let orig = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    let orig = RObject::from_value(orig).expect("the source answers a plain object");
    let dup = RObject::from_value(orig.dup(&mrb).expect("dup does not raise"))
        .expect("a plain object dups to a plain object");
    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    // The dup carries the copied ivar...
    assert_eq!(dup.ivar_get::<_, i32>(&mrb, x).ok(), Some(1));
    // ...and is a distinct object: mutating it leaves the original.
    dup.ivar_set(&mrb, x, 2i32)
        .expect("ivar_set on a fresh object does not raise");
    assert_eq!(dup.ivar_get::<_, i32>(&mrb, x).ok(), Some(2));
    assert_eq!(orig.ivar_get::<_, i32>(&mrb, x).ok(), Some(1));
}

#[test]
fn ivar_set_surfaces_a_frozen_receiver_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_set_test.rb").expect("allocating the context must succeed");
    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    let one = 1i32.into_value(&mrb);

    // A frozen receiver rejects the assignment — surfaced as Err
    // instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"Object.new.freeze")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let frozen = RObject::from_value(frozen).expect("the source answers a plain object");
    assert!(matches!(
        frozen.ivar_set(&mrb, x, one),
        Err(Error::Exception(_))
    ));
}

#[test]
fn const_get_reads_a_constant_and_surfaces_an_absent_one_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"const_get_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"module BeniConstHost; FOO = 7; end; BeniConstHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let module = RModule::from_value(module).expect("the source answers a module");

    // A defined constant reads back its value.
    let foo = mrb.intern_cstr(c"FOO").expect("the name interns");
    assert_eq!(
        module
            .const_get::<_, i32>(&mrb, foo)
            .expect("FOO is defined"),
        7
    );

    // An absent constant raises NameError — surfaced as Err instead
    // of unwinding across the call.
    let missing = mrb.intern_cstr(c"BENI_MISSING").expect("the name interns");
    assert!(matches!(
        module.const_get::<_, Value>(&mrb, missing),
        Err(Error::Exception(_))
    ));
}

#[test]
fn cvar_get_reads_a_class_variable_and_surfaces_an_absent_one_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cvar_get_test.rb").expect("allocating the context must succeed");

    let class = cxt
        .load_nstring(b"class BeniCvHost; @@count = 3; end; BeniCvHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let class = RClass::from_value(class).expect("the source answers a class");

    // A defined class variable reads back its value.
    let count = mrb.intern_cstr(c"@@count").expect("the name interns");
    assert_eq!(
        class
            .cvar_get::<_, i32>(&mrb, count)
            .expect("@@count is defined"),
        3
    );

    // An absent class variable raises NameError — surfaced as Err
    // instead of unwinding across the call.
    let missing = mrb
        .intern_cstr(c"@@beni_missing")
        .expect("the name interns");
    assert!(matches!(
        class.cvar_get::<_, Value>(&mrb, missing),
        Err(Error::Exception(_))
    ));
}

#[test]
fn const_set_assigns_a_constant() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"const_set_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"module BeniConstWriteHost; end; BeniConstWriteHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let module = RModule::from_value(module).expect("the source answers a module");

    // A fresh constant assigned on a module reads back its value.
    let bar = mrb.intern_cstr(c"BAR").expect("the name interns");
    module
        .const_set(&mrb, bar, 9i32)
        .expect("assigning a constant on a module must succeed");
    assert_eq!(
        module
            .const_get::<_, i32>(&mrb, bar)
            .expect("BAR was just set"),
        9
    );
}

#[test]
fn const_remove_removes_a_constant_and_treats_an_absent_one_as_a_no_op() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"const_remove_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"module BeniConstRemoveHost; GONE = 5; end; BeniConstRemoveHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let module = RModule::from_value(module).expect("the source answers a module");

    // Removing a defined constant succeeds and clears its presence.
    let gone = mrb.intern_cstr(c"GONE").expect("the name interns");
    assert!(module.const_defined(&mrb, gone), "GONE is defined");
    module
        .const_remove(&mrb, gone)
        .expect("removing a defined constant must succeed");
    assert!(
        !module.const_defined(&mrb, gone),
        "GONE is gone after removal"
    );

    // Removing an absent constant is a no-op, not an error.
    module
        .const_remove(&mrb, gone)
        .expect("removing an absent constant is a no-op");
}

#[test]
fn const_defined_at_answers_only_for_the_receivers_own_constant() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"const_defined_at_test.rb")
        .expect("allocating the context must succeed");

    let child = cxt
        .load_nstring(
            b"class BeniConstAtParent; OWNED = 1; end; \
          class BeniConstAtChild < BeniConstAtParent; end; BeniConstAtChild",
        )
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let child = RClass::from_value(child).expect("the source answers a class");

    let owned = mrb.intern_cstr(c"OWNED").expect("the name interns");
    let absent = mrb.intern_cstr(c"ABSENT").expect("the name interns");

    // A constant living only on the parent walks into reach for the
    // ancestry-walking test but stays invisible to the direct test.
    assert!(
        child.const_defined(&mrb, owned),
        "OWNED is reachable through the ancestry"
    );
    assert!(
        !child.const_defined_at(&mrb, owned),
        "OWNED is inherited, not on the child's own table"
    );

    // The constant on the receiver's own table is seen by the direct test.
    let parent = cxt
        .load_nstring(b"BeniConstAtParent")
        .expect("the test source must compile and run");
    let parent = RClass::from_value(parent).expect("the source answers a class");
    assert!(
        parent.const_defined_at(&mrb, owned),
        "OWNED is on the parent's own table"
    );

    // An absent constant is false either way — a total predicate.
    assert!(!child.const_defined_at(&mrb, absent), "ABSENT is undefined");
}

#[test]
fn cvar_set_assigns_a_class_variable_and_surfaces_a_frozen_receiver_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cvar_set_test.rb").expect("allocating the context must succeed");

    let class = cxt
        .load_nstring(b"class BeniCvWriteHost; end; BeniCvWriteHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let class = RClass::from_value(class).expect("the source answers a class");

    // A class variable assigned on a class reads back its value.
    let total = mrb.intern_cstr(c"@@total").expect("the name interns");
    class
        .cvar_set(&mrb, total, 5i32)
        .expect("assigning a class variable on a class must succeed");
    assert_eq!(
        class
            .cvar_get::<_, i32>(&mrb, total)
            .expect("@@total was just set"),
        5
    );

    // A frozen receiver rejects the assignment — surfaced as Err
    // instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"BeniCvWriteHost.freeze")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "freezing must not raise");
    let frozen = RClass::from_value(frozen).expect("the source answers a class");
    assert!(matches!(
        frozen.cvar_set(&mrb, total, 6i32),
        Err(Error::Exception(_))
    ));
}

#[test]
fn cvar_defined_tests_class_variable_presence_walking_the_ancestry() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cvar_defined_test.rb").expect("allocating the context must succeed");

    let child = cxt
        .load_nstring(
            b"class BeniCvParent; @@inherited = 1; end; \
          class BeniCvChild < BeniCvParent; end; BeniCvChild",
        )
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let child = RClass::from_value(child).expect("the source answers a class");

    // A class variable defined on an ancestor is present on the
    // child; an absent one is not — the predicate is total, raising
    // for neither.
    let inherited = mrb.intern_cstr(c"@@inherited").expect("the name interns");
    let missing = mrb.intern_cstr(c"@@missing").expect("the name interns");
    assert!(child.cvar_defined(&mrb, inherited));
    assert!(!child.cvar_defined(&mrb, missing));
}

#[test]
fn cvar_accessors_accept_a_singleton_class_receiver() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cvar_sclass_test.rb").expect("allocating the context must succeed");

    // A singleton class carries `MRB_TT_SCLASS`, inside the guarded
    // family: the accessors must keep working through it.
    let sclass = cxt
        .load_nstring(b"class BeniCvSclassHost; end; BeniCvSclassHost.singleton_class")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let sclass = RClass::from_value(sclass).expect("a singleton class is a class handle");

    let sym = mrb
        .intern_cstr(c"@@through_sclass")
        .expect("the name interns");
    sclass
        .cvar_set(&mrb, sym, 7i32)
        .expect("a singleton-class receiver must accept the write");
    let got: i32 = sclass
        .cvar_get(&mrb, sym)
        .expect("a singleton-class receiver must read the value back");
    assert_eq!(got, 7);
}

#[test]
fn ivar_defined_tests_instance_variable_presence() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_defined_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");

    // A set instance variable is present; an unset one is not — the
    // predicate is total, raising for neither.
    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    let y = mrb.intern_cstr(c"@y").expect("the name interns");
    assert!(obj.ivar_defined(&mrb, x));
    assert!(!obj.ivar_defined(&mrb, y));
}

#[test]
fn ivar_remove_yields_the_former_value_and_clears_presence() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_remove_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");

    // Removing a set variable hands back its former value and leaves
    // the variable undefined.
    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    let removed = obj.ivar_remove(&mrb, x).expect("removal does not raise");
    assert_eq!(removed.and_then(i32::from_value), Some(1));
    assert!(!obj.ivar_defined(&mrb, x));
}

#[test]
fn ivar_remove_distinguishes_absent_from_a_removed_nil() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ivar_remove_absent_test.rb")
        .expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, nil); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");

    // An absent variable yields None — distinct from a variable that
    // held nil, which yields Some(nil).
    let y = mrb.intern_cstr(c"@y").expect("the name interns");
    assert!(obj
        .ivar_remove(&mrb, y)
        .expect("absent removal does not raise")
        .is_none());

    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    let removed = obj.ivar_remove(&mrb, x).expect("removal does not raise");
    assert!(removed.is_some_and(Value::is_nil));
}

#[test]
fn ivar_remove_surfaces_a_frozen_holder_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ivar_remove_frozen_test.rb")
        .expect("allocating the context must succeed");

    // A frozen instance-variable holder rejects removal — surfaced as
    // Err instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o.freeze; o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let frozen = RObject::from_value(frozen).expect("the source answers a plain object");
    let x = mrb.intern_cstr(c"@x").expect("the name interns");
    assert!(matches!(
        frozen.ivar_remove(&mrb, x),
        Err(Error::Exception(_))
    ));
}

#[test]
fn clone_carries_frozen_state_where_dup_drops_it() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"clone_test.rb").expect("allocating the compile context must succeed");

    let frozen = cxt
        .load_nstring(b"Object.new.freeze")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // clone is the deeper copy — it preserves the frozen state;
    // dup always yields an unfrozen object.
    assert!(frozen
        .funcall(&mrb, c"clone", &[])
        .expect("clone does not raise")
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());
    assert!(!frozen
        .dup(&mrb)
        .expect("dup does not raise")
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());
}

#[test]
fn as_break_views_a_real_escaping_break() {
    let mrb = open_mrb();

    let class = mrb
        .define_class(c"BeniBreakYielder", mrb.object_class())
        .expect("defining the yielder class must succeed");
    class
        .define_method(&mrb, c"run", beni::method!(report_break, -1))
        .expect("registering the yielder method must succeed");

    let recv = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");
    mrb.gv_set(c"$beni_break_recv", recv)
        .expect("the name interns");

    // The block is captured via `&` so it stays non-orphan: `break
    // 88` surfaces as an RBreak the yielder catches, and `as_break`
    // reads its carried value back out.
    let cxt =
        Ccontext::new(&mrb, c"break_test.rb").expect("allocating the compile context must succeed");
    let got = cxt
        .load_nstring(b"$beni_break_recv.run(:tag) { break 88 }")
        .expect("the test source must compile and run");

    assert!(
        mrb.pending_exc().is_nil(),
        "the protected yield must not leave a pending exception: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(88));
}

#[test]
fn class_and_kind_predicates_read_the_hierarchy() {
    let mrb = open_mrb();
    let s = mrb.str_new(b"hi").as_value();
    let string_class = mrb.class_get(c"String").expect("String is defined");
    let object_class = mrb.class_get(c"Object").expect("Object is defined");

    // class() names the receiver's direct class.
    assert_eq!(s.class(&mrb).name(&mrb), "String");

    // is_kind_of holds for the direct class and its ancestors;
    // instance_of only for the direct class.
    assert!(s.is_kind_of(&mrb, string_class));
    assert!(s.is_kind_of(&mrb, object_class));
    assert!(s.is_instance_of(&mrb, string_class));
    assert!(!s.is_instance_of(&mrb, object_class));
}

#[test]
fn kind_predicates_take_a_module_through_the_ancestry() {
    let mrb = open_mrb();
    let tagged = mrb
        .load_string(
            b"module BeniKindTag; end; module BeniKindUnrelated; end
              class BeniKindTagged; include BeniKindTag; end
              BeniKindTagged.new",
        )
        .expect("the fixture loads");
    let tag = mrb
        .module_get(c"BeniKindTag")
        .expect("the module is defined");
    let unrelated = mrb
        .module_get(c"BeniKindUnrelated")
        .expect("the module is defined");

    // A module the class includes sits in the ancestry is_a? walks;
    // instance_of? matches only the class the value belongs to, which
    // a module never is.
    assert!(tagged.is_kind_of(&mrb, tag));
    assert!(!tagged.is_kind_of(&mrb, unrelated));
    assert!(!tagged.is_instance_of(&mrb, tag));
}

#[test]
fn kind_predicates_take_an_exception_class_handle() {
    let mrb = open_mrb();
    let argument_error = mrb
        .exc_get(c"ArgumentError")
        .expect("ArgumentError is built in");
    let standard_error = mrb
        .exc_get(c"StandardError")
        .expect("StandardError is built in");
    let exc = argument_error
        .new_str(&mrb, mrb.str_new("boom".as_bytes()))
        .as_value();

    assert!(exc.is_kind_of(&mrb, standard_error));
    assert!(exc.is_instance_of(&mrb, argument_error));
    assert!(!exc.is_instance_of(&mrb, standard_error));
}

#[test]
fn singleton_class_reads_a_stable_eigenclass() {
    let mrb = open_mrb();
    let s = RObject::from_value(
        mrb.object_class()
            .new_instance(&mrb, &[])
            .expect("Object.new constructs without raising"),
    )
    .expect("Object.new answers a plain object");

    // An ordinary object's singleton class is its own per-instance
    // eigenclass — distinct from the regular class it shares with peers.
    let sclass = s
        .singleton_class(&mrb)
        .expect("a plain object has a singleton class");
    assert!(!same_object(&mrb, sclass, s.class(&mrb)));

    // Re-reading the same object yields the same singleton class.
    let again = s
        .singleton_class(&mrb)
        .expect("a plain object has a singleton class");
    assert!(same_object(&mrb, sclass, again));
}

#[test]
fn freeze_marks_the_value_frozen() {
    let mrb = open_mrb();
    let s = mrb.str_new(b"x").as_value();
    assert!(!s
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());

    let frozen = s.freeze(&mrb);
    assert!(frozen
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());
}

#[test]
fn arithmetic_computes_on_integers_and_floats() {
    let mrb = open_mrb();

    // Integer operands yield an Integer result, like Ruby's 2 + 3 == 5.
    let sum = 2i32
        .into_value(&mrb)
        .add(&mrb, 3i32.into_value(&mrb))
        .expect("2 + 3 computes");
    assert_eq!(i32::from_value(sum), Some(5));
    // Subtraction and multiplication follow the same Integer path.
    let diff = 10i32
        .into_value(&mrb)
        .sub(&mrb, 4i32.into_value(&mrb))
        .expect("10 - 4 computes");
    assert_eq!(i32::from_value(diff), Some(6));
    let product = 6i32
        .into_value(&mrb)
        .mul(&mrb, 7i32.into_value(&mrb))
        .expect("6 * 7 computes");
    assert_eq!(i32::from_value(product), Some(42));
}

#[test]
fn arithmetic_widens_a_mixed_operand_to_float() {
    let mrb = open_mrb();

    // A float operand widens the result to a Float, like Ruby's
    // 2 + 3.5 == 5.5; f64::from_value reads only the Float tag, so a Some
    // confirms the result is a Float, not an Integer.
    let sum = 2i32
        .into_value(&mrb)
        .add(&mrb, 3.5f32.into_value(&mrb))
        .expect("2 + 3.5 computes");
    assert_eq!(f64::from_value(sum), Some(5.5));
    // The float receiver path widens the same way.
    let product = 1.5f32
        .into_value(&mrb)
        .mul(&mrb, 4i32.into_value(&mrb))
        .expect("1.5 * 4 computes");
    assert_eq!(f64::from_value(product), Some(6.0));
}

#[test]
fn arithmetic_rejects_a_non_numeric_operand() {
    let mrb = open_mrb();

    // mrb_num_add dispatches on the numeric tag: a non-numeric right
    // operand raises TypeError, caught into Err rather than long-jumping.
    assert!(matches!(
        1i32.into_value(&mrb).add(&mrb, Value::nil()),
        Err(Error::Exception(_))
    ));
    // A non-numeric receiver is rejected the same way.
    assert!(matches!(
        Value::nil().add(&mrb, 1i32.into_value(&mrb)),
        Err(Error::Exception(_))
    ));
    // The VM stays usable after the protected raise.
    assert_eq!(
        i32::from_value(
            1i32.into_value(&mrb)
                .add(&mrb, 1i32.into_value(&mrb))
                .expect("the VM survives the protected raise")
        ),
        Some(2)
    );
}

#[test]
fn arithmetic_surfaces_integer_overflow_as_err() {
    let mrb = open_mrb();

    // An integer result past the configured width has two lawful
    // outcomes, branched on the build's integer model rather than a
    // compile-time flag: a fixed-width config (no bigint) raises
    // RangeError, caught into Err; a bigint config promotes the result to
    // a BigInt and returns it. The bound is read from beni::sys::mrb_int so the
    // overflow is forced at any width.
    let max = beni::sys::mrb_int::MAX.into_value(&mrb);
    match max.add(&mrb, 1i32.into_value(&mrb)) {
        Err(Error::Exception(exc)) => {
            // The fixed-width lane stays strict: the surfaced exception is
            // exactly the RangeError the SPEC mandates for that config.
            assert_eq!(exc.classname(&mrb), "RangeError");
        }
        Ok(promoted) => {
            // The bigint lane lawfully promotes instead of raising; the
            // result keeps the Integer class (a BigInt is allocated on
            // mruby's integer_class), so the value stays a numeric
            // Integer rather than degrading to another type.
            assert_eq!(promoted.classname(&mrb), "Integer");
        }
        Err(other) => panic!("overflow must surface as a RangeError, got {other:?}"),
    }
}

#[test]
fn ivar_foreach_visits_every_set_instance_variable() {
    use beni::{ForEach, Symbol};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ivar_foreach.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@a").expect("the name interns"),
        1i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@b").expect("the name interns"),
        2i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@c").expect("the name interns"),
        3i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");

    let mut seen = Vec::new();
    obj.ivar_foreach(&mrb, |name: Symbol, val| {
        seen.push((
            name.name(&mrb).expect("an ivar name interns to a name"),
            i32::from_value(val).expect("the seeded values are integers"),
        ));
        ForEach::Continue
    });
    seen.sort();

    assert_eq!(
        seen,
        vec![
            ("@a".to_owned(), 1),
            ("@b".to_owned(), 2),
            ("@c".to_owned(), 3),
        ]
    );
}

#[test]
fn ivar_foreach_visits_nothing_for_a_receiver_without_instance_variables() {
    use beni::ForEach;

    let mrb = open_mrb();

    // A fresh object holds no instance variables, so the foreach
    // returns without ever calling back.
    let holder = RObject::from_value(
        mrb.object_class()
            .new_instance(&mrb, &[])
            .expect("Object.new constructs without raising"),
    )
    .expect("Object.new answers a plain object");
    let mut count = 0;
    holder.ivar_foreach(&mrb, |_, _| {
        count += 1;
        ForEach::Continue
    });
    assert_eq!(count, 0);
}

#[test]
fn ivar_foreach_stops_early_on_stop() {
    use beni::ForEach;

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_foreach_stop.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@a").expect("the name interns"),
        1i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@b").expect("the name interns"),
        2i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@c").expect("the name interns"),
        3i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");

    // Stopping at the first variable leaves the rest unvisited.
    let mut count = 0;
    obj.ivar_foreach(&mrb, |_, _| {
        count += 1;
        ForEach::Stop
    });

    assert_eq!(count, 1);
}

#[test]
fn ivar_foreach_visits_the_snapshot_when_the_closure_mutates_the_receiver() {
    use beni::{ForEach, Symbol};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"ivar_foreach_mutate.rb")
        .expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");
    let a = mrb.intern_cstr(c"@a").expect("the name interns");
    let b = mrb.intern_cstr(c"@b").expect("the name interns");
    let added = mrb.intern_cstr(c"@added").expect("the name interns");
    obj.ivar_set(&mrb, a, 1i32.into_value(&mrb))
        .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(&mrb, b, 2i32.into_value(&mrb))
        .expect("ivar_set on a fresh object does not raise");

    // The closure adds a variable and reassigns @b on every visit:
    // the mutations land on the receiver, while the iteration keeps
    // visiting the two variables and the values captured when it
    // began.
    let mut seen = Vec::new();
    obj.ivar_foreach(&mrb, |name: Symbol, val| {
        obj.ivar_set(&mrb, added, 9i32.into_value(&mrb))
            .expect("adding a variable mid-iteration lands on the receiver");
        obj.ivar_set(&mrb, b, 99i32.into_value(&mrb))
            .expect("reassigning a variable mid-iteration lands on the receiver");
        seen.push((
            name.name(&mrb).expect("an ivar name interns to a name"),
            i32::from_value(val).expect("the seeded values are integers"),
        ));
        ForEach::Continue
    });
    seen.sort();

    assert_eq!(seen, vec![("@a".to_owned(), 1), ("@b".to_owned(), 2)]);
    assert_eq!(obj.ivar_get::<_, i32>(&mrb, added).ok(), Some(9));
    assert_eq!(obj.ivar_get::<_, i32>(&mrb, b).ok(), Some(99));
}

#[test]
fn ivar_foreach_keeps_snapshot_values_alive_across_removal_and_gc() {
    use beni::{ForEach, RString};

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_foreach_gc.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");
    let a = mrb.intern_cstr(c"@a").expect("the name interns");
    let b = mrb.intern_cstr(c"@b").expect("the name interns");

    // Release the strings' creation-time arena slots so the
    // receiver's iv table is their only reference going into the
    // iteration.
    let scope = mrb.arena_scope();
    obj.ivar_set(&mrb, a, mrb.str_new(b"one").as_value())
        .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(&mrb, b, mrb.str_new(b"two").as_value())
        .expect("ivar_set on a fresh object does not raise");
    drop(scope);

    // The first visit removes every variable and runs a full
    // collection; the remaining snapshot value must still read
    // intact — the iteration owns its arena protection.
    let mut seen = Vec::new();
    let mut first = true;
    obj.ivar_foreach(&mrb, |_, val| {
        if first {
            first = false;
            obj.ivar_remove(&mrb, a).expect("removal does not raise");
            obj.ivar_remove(&mrb, b).expect("removal does not raise");
            mrb.full_gc();
        }
        let s = RString::from_value(val).expect("the seeded values are strings");
        seen.push(String::from_utf8(s.owned_bytes()).expect("the seeded bytes are UTF-8"));
        ForEach::Continue
    });
    seen.sort();

    assert_eq!(seen, vec!["one".to_owned(), "two".to_owned()]);
    assert!(!obj.ivar_defined(&mrb, a), "the removals landed");
    assert!(!obj.ivar_defined(&mrb, b), "the removals landed");
}

#[test]
fn ivar_foreach_resurfaces_a_closure_panic_on_the_rust_side() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"ivar_foreach_panic.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let obj = RObject::from_value(obj).expect("the source answers a plain object");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@a").expect("the name interns"),
        1i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");
    obj.ivar_set(
        &mrb,
        mrb.intern_cstr(c"@b").expect("the name interns"),
        2i32.into_value(&mrb),
    )
    .expect("ivar_set on a fresh object does not raise");

    // A panic in the closure ends the iteration and propagates on the
    // Rust side — the closure runs against the collected snapshot, so
    // no mruby C frame is on the stack to unwind through. catch_unwind
    // sees the panic with its payload intact.
    let visited = std::cell::Cell::new(0u32);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        obj.ivar_foreach(&mrb, |_, _| {
            visited.set(visited.get() + 1);
            panic!("boom in ivar_foreach closure");
        });
    }));

    let payload = result.expect_err("the closure panic must resurface Rust-side");
    let msg = payload
        .downcast_ref::<&str>()
        .copied()
        .expect("the original panic payload survives the round-trip");
    assert_eq!(msg, "boom in ivar_foreach closure");
    // The walk stopped at the first variable rather than running on.
    assert_eq!(visited.get(), 1);

    // The VM survives the caught panic.
    assert_eq!(
        obj.ivar_get::<_, i32>(&mrb, mrb.intern_cstr(c"@b").expect("the name interns"))
            .ok(),
        Some(2)
    );
}

#[test]
fn classname_survives_a_gc_cycle() {
    let mrb = open_mrb();

    // `classname` owns its bytes: mruby builds the name into a
    // GC-managed temporary, so a name held across a collection must
    // keep reading correctly rather than dangle into freed storage.
    let name = mrb.str_new(b"hello").as_value().classname(&mrb);
    mrb.full_gc();
    assert_eq!(name, "String");
}

#[test]
fn a_boxed_c_pointer_converts_into_rcptr_alone() {
    let mrb = open_mrb();
    let mut target = 0u8;
    // SAFETY: the interpreter is live; boxing an address reads nothing.
    let cptr = unsafe {
        <Value as beni::sys::FromRawValue>::from_raw(beni::sys::mrb_cptr_value(
            mrb.as_ptr(),
            (&mut target as *mut u8).cast(),
        ))
    };

    assert!(beni::RCptr::from_value(cptr).is_some());
    assert!(beni::RInlineStruct::from_value(cptr).is_none());
    assert!(beni::RCptr::from_value(Value::nil()).is_none());
    assert!(beni::RCptr::from_value(mrb.object_class().as_value()).is_none());
}

/// Assign and read back one instance variable through `holder`.
fn keeps_an_instance_variable<T: beni::Object>(mrb: &Mrb, holder: T) -> Option<i32> {
    let name = mrb.intern_cstr(c"@kept").expect("the name interns");
    holder
        .ivar_set(mrb, name, 5i32)
        .expect("assigning on an unfrozen holder does not raise");
    holder.ivar_get::<_, i32>(mrb, name).ok()
}

#[test]
fn every_instance_variable_holder_keeps_an_instance_variable() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"holders.rb").expect("allocating the context must succeed");
    let eval = |src: &[u8]| cxt.load_nstring(src).expect("the source evaluates");

    let object = RObject::from_value(eval(b"Object.new")).expect("a plain object");
    let class = RClass::from_value(eval(b"Class.new")).expect("a class");
    let module = RModule::from_value(eval(b"Module.new")).expect("a module");
    let hash = beni::RHash::from_value(eval(b"{}")).expect("a hash");
    let exception = beni::Exception::from_value(eval(b"RuntimeError.new")).expect("an exception");
    let exception_class = mrb
        .exc_get(c"RuntimeError")
        .expect("RuntimeError is defined");

    assert_eq!(keeps_an_instance_variable(&mrb, object), Some(5));
    assert_eq!(keeps_an_instance_variable(&mrb, class), Some(5));
    assert_eq!(keeps_an_instance_variable(&mrb, module), Some(5));
    assert_eq!(keeps_an_instance_variable(&mrb, hash), Some(5));
    assert_eq!(keeps_an_instance_variable(&mrb, exception), Some(5));
    assert_eq!(keeps_an_instance_variable(&mrb, exception_class), Some(5));
}
