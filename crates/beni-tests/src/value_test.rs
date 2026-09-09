use crate::support::open_mrb;
use beni::state::args::format;
use beni::{Ccontext, Error, FromValue, IntoValue, Module, Mrb, Proc, Symbol, Value};

/// Yielder method in the boundary-terminating shape kobako uses:
/// read the captured (non-orphan) block, yield it, and on a real
/// `break` report its carried value back as the method's result.
fn report_break(mrb: &Mrb, _self: Value) -> Value {
    let (_sym, _rest, block_val) = mrb.get_args::<format::NRestBlock>();
    let block = Proc::from_value(block_val).expect("the captured block is a Proc");
    match block.call(mrb, &[]) {
        Ok(_) => Value::from_int(mrb, -1),
        Err(Error::Exception(exc)) => match exc.as_break() {
            Some(brk) => brk.value(),
            None => Value::from_int(mrb, -2),
        },
        Err(_) => Value::from_int(mrb, -3),
    }
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
    // equivalent name, proving the `IntoSym` generalization routes
    // both through the same interned symbol.
    let recv = 42i32.into_value(&mrb);
    let by_name = recv
        .funcall(&mrb, c"to_s", &[])
        .expect("the name key must dispatch");
    let by_sym = recv
        .funcall(&mrb, Symbol::new(&mrb, c"to_s"), &[])
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
fn is_string_discriminates_the_string_tag() {
    let mrb = open_mrb();

    assert!(mrb.str_new(b"x").as_value().is_string());
    // A non-String tag — and an immediate — both reject.
    assert!(!42i32.into_value(&mrb).is_string());
    assert!(!Value::nil().is_string());
}

#[test]
fn tag_predicates_discriminate_module_range_and_exception() {
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

    // Each predicate holds for exactly its own tag.
    assert!(module.is_module());
    assert!(range.is_range());
    assert!(exception.is_exception());

    // A class is not a module: is_class and is_module split the
    // class-family tags, and neither claims the other's value.
    let class = cxt
        .load_nstring(b"String")
        .expect("the test source must compile and run");
    assert!(class.is_class());
    assert!(!class.is_module());
    assert!(!module.is_class());

    // No predicate claims an unrelated tag, nor an immediate.
    assert!(!range.is_module());
    assert!(!exception.is_range());
    assert!(!42i32.into_value(&mrb).is_exception());
    assert!(!Value::nil().is_module());
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
    let rendered = obj.any_to_s(&mrb).to_bytes();
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
    assert!(a.obj_equal(&mrb, a));
    assert!(!a.obj_equal(&mrb, b));
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
    assert!(matches!(obj.obj_dup(&mrb), Err(Error::Exception(_))));
    assert!(matches!(obj.obj_clone(&mrb), Err(Error::Exception(_))));
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
fn obj_as_string_coerces_through_to_s() {
    let mrb = open_mrb();

    // Already a string: coercion returns that same string, not a copy.
    let already = mrb.str_new(b"hi").as_value();
    let coerced = already
        .obj_as_string(&mrb)
        .expect("a string coerces without raising");
    assert!(coerced.is_string());
    assert!(already.obj_equal(&mrb, coerced));

    // A non-string coerces through its `to_s`.
    assert!(42i32
        .into_value(&mrb)
        .obj_as_string(&mrb)
        .expect("to_s of an integer does not raise")
        .is_string());
}

#[test]
fn ensure_string_returns_the_handle_or_raises_by_tag() {
    let mrb = open_mrb();

    // A String tag yields the same string as a typed handle — no
    // copy, no dispatch.
    let s = mrb.str_new(b"hi").as_value();
    let handle = s
        .ensure_string(&mrb)
        .expect("a String value coerces without raising");
    assert!(s.obj_equal(&mrb, handle.as_value()));
    assert_eq!(handle.to_bytes(), b"hi".to_vec());

    // A non-String tag raises `TypeError` rather than coercing — the
    // contrast with `obj_as_string`, which would render the integer
    // through `to_s`. The raise is the genuine `TypeError` class, not
    // some other exception.
    match 42i32.into_value(&mrb).ensure_string(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-String value surfaces a TypeError Err"),
    }
}

#[test]
fn ensure_array_returns_the_handle_or_raises_by_tag() {
    let mrb = open_mrb();

    // An Array tag yields the same array as a typed handle — no
    // copy, no dispatch.
    let a = mrb.ary_new().as_value();
    let handle = a
        .ensure_array(&mrb)
        .expect("an Array value coerces without raising");
    assert!(a.obj_equal(&mrb, handle.as_value()));

    // A non-Array tag raises `TypeError` rather than coercing — no
    // `to_ary` dispatch. The raise is the genuine `TypeError`
    // class, not some other exception.
    match 42i32.into_value(&mrb).ensure_array(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-Array value surfaces a TypeError Err"),
    }
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
    let spread = src
        .as_value()
        .to_ary(&mrb)
        .expect("an array spreads without raising");
    assert_eq!(spread.len(), 2);
    assert!(!src.as_value().obj_equal(&mrb, spread.as_value()));

    // A scalar that does not respond to `to_a` wraps in `[scalar]`.
    let wrapped = 7i32
        .into_value(&mrb)
        .to_ary(&mrb)
        .expect("a scalar wraps without raising");
    assert_eq!(wrapped.len(), 1);
    assert_eq!(i32::from_value(wrapped.entry(0)), Some(7));

    // `nil` answers `to_a` with an empty array here (mruby-object-ext
    // defines `NilClass#to_a`), so it spreads to `[]` — the responder
    // path, not a wrap.
    let nil_spread = Value::nil()
        .to_ary(&mrb)
        .expect("nil spreads through its to_a");
    assert_eq!(nil_spread.len(), 0);

    // A `to_a` responder whose result is an array passes that array
    // through: a Range yields its enumerated elements.
    let range = mrb
        .range_new(1i32.into_value(&mrb), 3i32.into_value(&mrb), false)
        .expect("a Range over comparable bounds constructs");
    let enumerated = range
        .as_value()
        .to_ary(&mrb)
        .expect("a Range spreads through its to_a");
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
        .obj_new(&mrb, &[])
        .expect("the class whose to_a returns nil instantiates");
    let nil_returned = nil_obj
        .to_ary(&mrb)
        .expect("a nil-returning to_a wraps without raising");
    assert_eq!(nil_returned.len(), 1);
    assert!(nil_obj.obj_equal(&mrb, nil_returned.entry(0)));

    // A `to_a` that returns a non-array non-`nil` value raises a
    // genuine `TypeError`, caught into the `Err` rather than wrapping.
    let class = mrb
        .class_new(mrb.object_class())
        .expect("an anonymous class under Object constructs");
    class
        .define_method(&mrb, c"to_a", beni::method!(to_a_returns_int, 0))
        .expect("registering to_a must succeed");
    let obj = class
        .obj_new(&mrb, &[])
        .expect("the class with a misbehaving to_a instantiates");
    match obj.to_ary(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-array non-nil to_a surfaces a TypeError Err"),
    }
}

#[test]
fn ensure_hash_returns_the_handle_or_raises_by_tag() {
    let mrb = open_mrb();

    // A Hash tag yields the same hash as a typed handle — no copy,
    // no dispatch.
    let h = mrb.hash_new().as_value();
    let handle = h
        .ensure_hash(&mrb)
        .expect("a Hash value coerces without raising");
    assert!(h.obj_equal(&mrb, handle.as_value()));

    // A non-Hash tag raises `TypeError` rather than coercing — no
    // `to_hash` dispatch. The raise is the genuine `TypeError`
    // class, not some other exception.
    match 42i32.into_value(&mrb).ensure_hash(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-Hash value surfaces a TypeError Err"),
    }
}

#[test]
fn bool_predicates_separate_true_false_and_nil() {
    // The immediate singletons need a live VM to have been captured,
    // even though the predicates themselves take no `Mrb`.
    let _mrb = open_mrb();

    // `is_true` / `is_false` are exact: each admits only its own
    // singleton. The load-bearing case is that `nil` — which shares
    // the false tag under some boxing modes — is neither.
    assert!(Value::true_().is_true());
    assert!(!Value::true_().is_false());
    assert!(Value::false_().is_false());
    assert!(!Value::false_().is_true());
    assert!(!Value::nil().is_true());
    assert!(!Value::nil().is_false());
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
fn obj_dup_copies_state_into_an_independent_object() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"dup_test.rb").expect("allocating the compile context must succeed");

    let orig = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    let dup = orig.obj_dup(&mrb).expect("dup does not raise");
    let x = mrb.intern_cstr(c"@x");
    // The dup carries the copied ivar...
    assert_eq!(i32::from_value(dup.iv_get(&mrb, x)), Some(1));
    // ...and is a distinct object: mutating it leaves the original.
    dup.iv_set(&mrb, x, 2i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    assert_eq!(i32::from_value(dup.iv_get(&mrb, x)), Some(2));
    assert_eq!(i32::from_value(orig.iv_get(&mrb, x)), Some(1));
}

#[test]
fn iv_set_surfaces_frozen_and_non_object_receivers_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"iv_set_test.rb").expect("allocating the context must succeed");
    let x = mrb.intern_cstr(c"@x");
    let one = 1i32.into_value(&mrb);

    // A frozen receiver rejects the assignment — surfaced as Err
    // instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"Object.new.freeze")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    assert!(matches!(
        frozen.iv_set(&mrb, x, one),
        Err(Error::Exception(_))
    ));

    // An immediate cannot hold instance variables — also an Err, not UB.
    assert!(matches!(
        42i32.into_value(&mrb).iv_set(&mrb, x, one),
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

    // A defined constant reads back its value.
    let foo = mrb.intern_cstr(c"FOO");
    assert_eq!(
        i32::from_value(module.const_get(&mrb, foo).expect("FOO is defined")),
        Some(7)
    );

    // An absent constant raises NameError — surfaced as Err instead
    // of unwinding across the call.
    let missing = mrb.intern_cstr(c"BENI_MISSING");
    assert!(matches!(
        module.const_get(&mrb, missing),
        Err(Error::Exception(_))
    ));
}

#[test]
fn cv_get_reads_a_class_variable_and_surfaces_an_absent_one_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"cv_get_test.rb").expect("allocating the context must succeed");

    let class = cxt
        .load_nstring(b"class BeniCvHost; @@count = 3; end; BeniCvHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // A defined class variable reads back its value.
    let count = mrb.intern_cstr(c"@@count");
    assert_eq!(
        i32::from_value(class.cv_get(&mrb, count).expect("@@count is defined")),
        Some(3)
    );

    // An absent class variable raises NameError — surfaced as Err
    // instead of unwinding across the call.
    let missing = mrb.intern_cstr(c"@@beni_missing");
    assert!(matches!(
        class.cv_get(&mrb, missing),
        Err(Error::Exception(_))
    ));
}

#[test]
fn const_set_assigns_a_constant_and_surfaces_a_non_module_receiver_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"const_set_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"module BeniConstWriteHost; end; BeniConstWriteHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // A fresh constant assigned on a module reads back its value.
    let bar = mrb.intern_cstr(c"BAR");
    module
        .const_set(&mrb, bar, 9i32.into_value(&mrb))
        .expect("assigning a constant on a module must succeed");
    assert_eq!(
        i32::from_value(module.const_get(&mrb, bar).expect("BAR was just set")),
        Some(9)
    );

    // A non-module receiver raises TypeError — surfaced as Err
    // instead of unwinding across the call.
    assert!(matches!(
        42i32.into_value(&mrb).const_set(&mrb, bar, Value::nil()),
        Err(Error::Exception(_))
    ));
}

#[test]
fn const_remove_removes_a_constant_and_surfaces_a_non_module_receiver_as_err() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"const_remove_test.rb").expect("allocating the context must succeed");

    let module = cxt
        .load_nstring(b"module BeniConstRemoveHost; GONE = 5; end; BeniConstRemoveHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // Removing a defined constant succeeds and clears its presence.
    let gone = mrb.intern_cstr(c"GONE");
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

    // A non-module receiver raises TypeError — surfaced as Err
    // instead of unwinding across the call.
    assert!(matches!(
        42i32.into_value(&mrb).const_remove(&mrb, gone),
        Err(Error::Exception(_))
    ));
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

    let owned = mrb.intern_cstr(c"OWNED");
    let absent = mrb.intern_cstr(c"ABSENT");

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
    assert!(
        parent.const_defined_at(&mrb, owned),
        "OWNED is on the parent's own table"
    );

    // An absent constant is false either way — a total predicate.
    assert!(!child.const_defined_at(&mrb, absent), "ABSENT is undefined");
}

#[test]
fn cv_set_assigns_a_class_variable_and_surfaces_a_frozen_receiver_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"cv_set_test.rb").expect("allocating the context must succeed");

    let class = cxt
        .load_nstring(b"class BeniCvWriteHost; end; BeniCvWriteHost")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // A class variable assigned on a class reads back its value.
    let total = mrb.intern_cstr(c"@@total");
    class
        .cv_set(&mrb, total, 5i32.into_value(&mrb))
        .expect("assigning a class variable on a class must succeed");
    assert_eq!(
        i32::from_value(class.cv_get(&mrb, total).expect("@@total was just set")),
        Some(5)
    );

    // A frozen receiver rejects the assignment — surfaced as Err
    // instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"BeniCvWriteHost.freeze")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "freezing must not raise");
    assert!(matches!(
        frozen.cv_set(&mrb, total, 6i32.into_value(&mrb)),
        Err(Error::Exception(_))
    ));
}

#[test]
fn cv_defined_tests_class_variable_presence_walking_the_ancestry() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cv_defined_test.rb").expect("allocating the context must succeed");

    let child = cxt
        .load_nstring(
            b"class BeniCvParent; @@inherited = 1; end; \
          class BeniCvChild < BeniCvParent; end; BeniCvChild",
        )
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // A class variable defined on an ancestor is present on the
    // child; an absent one is not — the predicate is total, raising
    // for neither.
    let inherited = mrb.intern_cstr(c"@@inherited");
    let missing = mrb.intern_cstr(c"@@missing");
    assert!(child.cv_defined(&mrb, inherited));
    assert!(!child.cv_defined(&mrb, missing));
}

/// The `Err` must carry a `TypeError` — the same rejection the
/// constant accessors surface for a non-class receiver.
fn assert_type_error(mrb: &Mrb, err: Error) {
    match err {
        Error::Exception(exc) => assert_eq!(exc.classname(mrb), "TypeError"),
        other => panic!("expected a TypeError exception, got a panic, got {other}"),
    }
}

#[test]
fn cv_accessors_reject_a_receiver_that_is_not_a_class_or_module() {
    let mrb = open_mrb();
    let sym = mrb.intern_cstr(c"@@x");

    // nil, an immediate, and a plain object all sit outside the
    // class-or-module family the accessors dereference into: the
    // reads and writes surface a TypeError `Err`, the presence
    // test stays a total predicate answering false.
    let receivers = [
        Value::nil(),
        Value::from_int(&mrb, 5),
        mrb.str_new(b"not a module").as_value(),
    ];
    for receiver in receivers {
        let err = receiver
            .cv_get(&mrb, sym)
            .expect_err("a non-class receiver must not read a class variable");
        assert_type_error(&mrb, err);

        let err = receiver
            .cv_set(&mrb, sym, Value::nil())
            .expect_err("a non-class receiver must not assign a class variable");
        assert_type_error(&mrb, err);

        assert!(!receiver.cv_defined(&mrb, sym));
    }
    // The VM stays usable after the rejections.
    assert!(mrb.pending_exc().is_nil());
}

#[test]
fn const_presence_answers_false_for_a_receiver_that_is_not_a_class_or_module() {
    let mrb = open_mrb();
    let sym = mrb.intern_cstr(c"X");

    // Both presence tests are total predicates: a receiver outside
    // the class-or-module family answers false instead of walking
    // an unchecked dereference.
    let receivers = [
        Value::nil(),
        Value::from_int(&mrb, 5),
        mrb.str_new(b"not a module").as_value(),
    ];
    for receiver in receivers {
        assert!(!receiver.const_defined(&mrb, sym));
        assert!(!receiver.const_defined_at(&mrb, sym));
    }
}

#[test]
fn cv_accessors_accept_a_singleton_class_receiver() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"cv_sclass_test.rb").expect("allocating the context must succeed");

    // A singleton class carries `MRB_TT_SCLASS`, inside the guarded
    // family: the accessors must keep working through it.
    let sclass = cxt
        .load_nstring(b"class BeniCvSclassHost; end; BeniCvSclassHost.singleton_class")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    let sym = mrb.intern_cstr(c"@@through_sclass");
    sclass
        .cv_set(&mrb, sym, Value::from_int(&mrb, 7))
        .expect("a singleton-class receiver must accept the write");
    let got = sclass
        .cv_get(&mrb, sym)
        .expect("a singleton-class receiver must read the value back");
    assert_eq!(i32::from_value(got), Some(7));
}

#[test]
fn iv_defined_tests_instance_variable_presence() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"iv_defined_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // A set instance variable is present; an unset one is not — the
    // predicate is total, raising for neither.
    let x = mrb.intern_cstr(c"@x");
    let y = mrb.intern_cstr(c"@y");
    assert!(obj.iv_defined(&mrb, x));
    assert!(!obj.iv_defined(&mrb, y));
}

#[test]
fn iv_remove_yields_the_former_value_and_clears_presence() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"iv_remove_test.rb").expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // Removing a set variable hands back its former value and leaves
    // the variable undefined.
    let x = mrb.intern_cstr(c"@x");
    let removed = obj.iv_remove(&mrb, x).expect("removal does not raise");
    assert_eq!(removed.and_then(i32::from_value), Some(1));
    assert!(!obj.iv_defined(&mrb, x));
}

#[test]
fn iv_remove_distinguishes_absent_from_a_removed_nil() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"iv_remove_absent_test.rb")
        .expect("allocating the context must succeed");

    let obj = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, nil); o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");

    // An absent variable yields None — distinct from a variable that
    // held nil, which yields Some(nil).
    let y = mrb.intern_cstr(c"@y");
    assert!(obj
        .iv_remove(&mrb, y)
        .expect("absent removal does not raise")
        .is_none());

    let x = mrb.intern_cstr(c"@x");
    let removed = obj.iv_remove(&mrb, x).expect("removal does not raise");
    assert!(removed.is_some_and(Value::is_nil));

    // An immediate cannot hold instance variables — also None, not Err.
    assert!(42i32
        .into_value(&mrb)
        .iv_remove(&mrb, x)
        .expect("a non-holder removal does not raise")
        .is_none());
}

#[test]
fn iv_remove_surfaces_a_frozen_holder_as_err() {
    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"iv_remove_frozen_test.rb")
        .expect("allocating the context must succeed");

    // A frozen instance-variable holder rejects removal — surfaced as
    // Err instead of unwinding across the call.
    let frozen = cxt
        .load_nstring(b"o = Object.new; o.instance_variable_set(:@x, 1); o.freeze; o")
        .expect("the test source must compile and run");
    assert!(mrb.pending_exc().is_nil(), "setup must not raise");
    let x = mrb.intern_cstr(c"@x");
    assert!(matches!(
        frozen.iv_remove(&mrb, x),
        Err(Error::Exception(_))
    ));
}

#[test]
fn obj_clone_carries_frozen_state_where_dup_drops_it() {
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
        .obj_clone(&mrb)
        .expect("clone does not raise")
        .funcall(&mrb, c"frozen?", &[])
        .expect("frozen? does not raise")
        .to_bool());
    assert!(!frozen
        .obj_dup(&mrb)
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
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_break_recv");
    mrb.gv_set(slot, recv);

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
fn singleton_class_reads_a_stable_eigenclass_and_rejects_immediates() {
    let mrb = open_mrb();
    let s = mrb.str_new(b"hi").as_value();

    // An ordinary object's singleton class is its own per-instance
    // eigenclass — distinct from the regular class it shares with peers.
    let sclass = s
        .singleton_class(&mrb)
        .expect("a string has a singleton class");
    assert_ne!(sclass.as_raw(), s.class(&mrb).as_raw());

    // Re-reading the same object yields the same singleton class.
    let again = s
        .singleton_class(&mrb)
        .expect("a string has a singleton class");
    assert_eq!(sclass.as_raw(), again.as_raw());

    // nil yields its predefined class, which acts as its singleton
    // class, so the read succeeds.
    assert_eq!(
        Value::nil()
            .singleton_class(&mrb)
            .expect("nil has a singleton class")
            .as_raw(),
        Value::nil().class(&mrb).as_raw()
    );

    // Every other immediate has no singleton class: the TypeError
    // mruby raises surfaces as Err.
    match Value::from_int(&mrb, 1).singleton_class(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        other => panic!("expected a TypeError Err, got {other:?}"),
    }
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
fn as_int_converts_across_numeric_types_and_surfaces_non_numeric_as_err() {
    let mrb = open_mrb();

    // An Integer reads directly.
    assert_eq!(
        42i32
            .into_value(&mrb)
            .as_int(&mrb)
            .expect("an Integer converts"),
        42
    );

    // A Float converts by truncating toward zero — unlike the
    // exact-tag `i32::from_value`, which rejects the Float tag.
    let float_val = 2.9f64.into_value(&mrb);
    assert_eq!(i32::from_value(float_val), None);
    assert_eq!(float_val.as_int(&mrb).expect("a Float truncates"), 2);

    // A non-numeric value raises TypeError — surfaced as Err instead
    // of unwinding across the call.
    assert!(matches!(
        mrb.str_new(b"x").as_value().as_int(&mrb),
        Err(Error::Exception(_))
    ));
}

#[test]
fn as_float_converts_across_numeric_types_and_surfaces_non_numeric_as_err() {
    let mrb = open_mrb();

    // A Float reads directly.
    assert_eq!(
        1.5f64
            .into_value(&mrb)
            .as_float(&mrb)
            .expect("a Float converts"),
        1.5
    );

    // An Integer widens to a float — unlike the exact-tag
    // `f64::from_value`, which rejects the Integer tag.
    let int_val = 3i32.into_value(&mrb);
    assert_eq!(f64::from_value(int_val), None);
    assert_eq!(int_val.as_float(&mrb).expect("an Integer widens"), 3.0);

    // A non-numeric value raises TypeError — surfaced as Err.
    assert!(matches!(
        mrb.str_new(b"x").as_value().as_float(&mrb),
        Err(Error::Exception(_))
    ));
}

#[test]
fn int_to_str_renders_in_base_ten_and_other_radixes() {
    let mrb = open_mrb();

    let n = Value::from_int(&mrb, 12345);
    // Base 10 is the plain decimal rendering.
    assert_eq!(
        n.int_to_str(&mrb, 10).expect("base 10 renders").to_bytes(),
        b"12345".to_vec()
    );
    // A non-decimal radix renders in that base, like Ruby's
    // 12345.to_s(16) == "3039".
    assert_eq!(
        n.int_to_str(&mrb, 16).expect("base 16 renders").to_bytes(),
        b"3039".to_vec()
    );
}

#[test]
fn int_to_str_surfaces_an_invalid_radix_as_err() {
    let mrb = open_mrb();

    // A radix outside 2 through 36 raises ArgumentError, caught into
    // Err rather than long-jumping; the VM stays usable afterward.
    assert!(matches!(
        Value::from_int(&mrb, 12345).int_to_str(&mrb, 1),
        Err(Error::Exception(_))
    ));
    assert_eq!(
        Value::from_int(&mrb, 42)
            .int_to_str(&mrb, 10)
            .expect("the VM survives the protected raise")
            .to_bytes(),
        b"42".to_vec()
    );
}

#[test]
fn int_to_str_rejects_a_non_integer_receiver() {
    let mrb = open_mrb();

    // The guard is strict on the Integer tag: a Float is rejected with
    // TypeError, not coerced, because mrb_integer_to_str unboxes its
    // receiver without a tag check.
    assert!(matches!(
        1.5f64.into_value(&mrb).int_to_str(&mrb, 10),
        Err(Error::Exception(_))
    ));
}

#[test]
fn float_to_int_truncates_toward_zero() {
    let mrb = open_mrb();

    // A positive float truncates down, like Ruby's 3.9.to_i == 3.
    let three = Value::from_float(&mrb, 3.9)
        .float_to_int(&mrb)
        .expect("3.9 converts");
    assert_eq!(i32::from_value(three), Some(3));
    // A negative float truncates toward zero, like Ruby's -3.9.to_i == -3.
    let neg_three = Value::from_float(&mrb, -3.9)
        .float_to_int(&mrb)
        .expect("-3.9 converts");
    assert_eq!(i32::from_value(neg_three), Some(-3));
}

#[test]
fn float_to_int_surfaces_infinity_and_nan_as_err() {
    let mrb = open_mrb();

    // Infinity and NaN have no integer; mruby raises RangeError, caught
    // into Err rather than long-jumping, and the VM stays usable after.
    assert!(matches!(
        Value::from_float(&mrb, f64::INFINITY).float_to_int(&mrb),
        Err(Error::Exception(_))
    ));
    assert!(matches!(
        Value::from_float(&mrb, f64::NAN).float_to_int(&mrb),
        Err(Error::Exception(_))
    ));
    assert_eq!(
        i32::from_value(
            Value::from_float(&mrb, 2.5)
                .float_to_int(&mrb)
                .expect("the VM survives the protected raise")
        ),
        Some(2)
    );
}

#[test]
fn float_to_int_rejects_a_non_float_receiver() {
    let mrb = open_mrb();

    // mrb_float_to_integer guards its receiver on the Float tag: an
    // Integer is rejected with TypeError, not passed through.
    assert!(matches!(
        Value::from_int(&mrb, 7).float_to_int(&mrb),
        Err(Error::Exception(_))
    ));
}

#[test]
fn ensure_int_coerces_by_numeric_type_or_raises() {
    let mrb = open_mrb();

    // An Integer coerces unchanged, staying an Integer value.
    let same = Value::from_int(&mrb, 5)
        .ensure_int(&mrb)
        .expect("an Integer coerces without raising");
    assert!(same.is_integer());
    assert_eq!(i32::from_value(same), Some(5));

    // A Float coerces by truncating toward zero, like Ruby's
    // Integer(-3.9) == -3 — the cross-numeric case.
    let truncated = Value::from_float(&mrb, -3.9)
        .ensure_int(&mrb)
        .expect("a Float coerces by truncation");
    assert!(truncated.is_integer());
    assert_eq!(i32::from_value(truncated), Some(-3));

    // An infinite or NaN Float has no integer; mruby raises RangeError,
    // caught into Err, and the VM stays usable.
    assert!(matches!(
        Value::from_float(&mrb, f64::INFINITY).ensure_int(&mrb),
        Err(Error::Exception(_))
    ));

    // A non-numeric value raises the genuine TypeError class rather than
    // coercing — no to_int dispatch.
    match mrb.str_new(b"7").as_value().ensure_int(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-numeric value surfaces a TypeError Err"),
    }
}

#[test]
fn ensure_float_coerces_by_numeric_type_or_raises() {
    let mrb = open_mrb();

    // A Float coerces unchanged, staying a Float value.
    let same = Value::from_float(&mrb, 2.5)
        .ensure_float(&mrb)
        .expect("a Float coerces without raising");
    assert!(same.is_float());
    assert_eq!(f64::from_value(same), Some(2.5));

    // An Integer widens to a Float — the cross-numeric case.
    let widened = Value::from_int(&mrb, 7)
        .ensure_float(&mrb)
        .expect("an Integer widens to a Float");
    assert!(widened.is_float());
    assert_eq!(f64::from_value(widened), Some(7.0));

    // A non-numeric value raises the genuine TypeError class rather than
    // coercing — no to_f dispatch.
    match mrb.str_new(b"2.5").as_value().ensure_float(&mrb) {
        Err(Error::Exception(exc)) => {
            assert_eq!(exc.class(&mrb).name(&mrb), "TypeError");
        }
        _ => panic!("a non-numeric value surfaces a TypeError Err"),
    }
}

#[test]
fn arithmetic_computes_on_integers_and_floats() {
    let mrb = open_mrb();

    // Integer operands yield an Integer result, like Ruby's 2 + 3 == 5.
    let sum = Value::from_int(&mrb, 2)
        .add(&mrb, Value::from_int(&mrb, 3))
        .expect("2 + 3 computes");
    assert_eq!(i32::from_value(sum), Some(5));
    // Subtraction and multiplication follow the same Integer path.
    let diff = Value::from_int(&mrb, 10)
        .sub(&mrb, Value::from_int(&mrb, 4))
        .expect("10 - 4 computes");
    assert_eq!(i32::from_value(diff), Some(6));
    let product = Value::from_int(&mrb, 6)
        .mul(&mrb, Value::from_int(&mrb, 7))
        .expect("6 * 7 computes");
    assert_eq!(i32::from_value(product), Some(42));
}

#[test]
fn arithmetic_widens_a_mixed_operand_to_float() {
    let mrb = open_mrb();

    // A float operand widens the result to a Float, like Ruby's
    // 2 + 3.5 == 5.5; f64::from_value reads only the Float tag, so a Some
    // confirms the result is a Float, not an Integer.
    let sum = Value::from_int(&mrb, 2)
        .add(&mrb, Value::from_float(&mrb, 3.5))
        .expect("2 + 3.5 computes");
    assert_eq!(f64::from_value(sum), Some(5.5));
    // The float receiver path widens the same way.
    let product = Value::from_float(&mrb, 1.5)
        .mul(&mrb, Value::from_int(&mrb, 4))
        .expect("1.5 * 4 computes");
    assert_eq!(f64::from_value(product), Some(6.0));
}

#[test]
fn arithmetic_rejects_a_non_numeric_operand() {
    let mrb = open_mrb();

    // mrb_num_add dispatches on the numeric tag: a non-numeric right
    // operand raises TypeError, caught into Err rather than long-jumping.
    assert!(matches!(
        Value::from_int(&mrb, 1).add(&mrb, Value::nil()),
        Err(Error::Exception(_))
    ));
    // A non-numeric receiver is rejected the same way.
    assert!(matches!(
        Value::nil().add(&mrb, Value::from_int(&mrb, 1)),
        Err(Error::Exception(_))
    ));
    // The VM stays usable after the protected raise.
    assert_eq!(
        i32::from_value(
            Value::from_int(&mrb, 1)
                .add(&mrb, Value::from_int(&mrb, 1))
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
    let max = Value::from_int(&mrb, beni::sys::mrb_int::MAX);
    match max.add(&mrb, Value::from_int(&mrb, 1)) {
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
fn each_iv_visits_every_set_instance_variable() {
    use beni::{ForEach, Symbol};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"each_iv.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@c"), 3i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");

    let mut seen = Vec::new();
    obj.each_iv(&mrb, |name: Symbol, val| {
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
fn each_iv_visits_nothing_for_a_receiver_without_instance_variables() {
    use beni::ForEach;

    let mrb = open_mrb();

    // An immediate cannot hold instance variables, so the guarded
    // foreach returns without ever calling back.
    let mut count = 0;
    Value::from_int(&mrb, 42).each_iv(&mrb, |_, _| {
        count += 1;
        ForEach::Continue
    });
    assert_eq!(count, 0);
}

#[test]
fn each_iv_stops_early_on_stop() {
    use beni::ForEach;

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"each_iv_stop.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@c"), 3i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");

    // Stopping at the first variable leaves the rest unvisited.
    let mut count = 0;
    obj.each_iv(&mrb, |_, _| {
        count += 1;
        ForEach::Stop
    });

    assert_eq!(count, 1);
}

#[test]
fn each_iv_visits_the_snapshot_when_the_closure_mutates_the_receiver() {
    use beni::{ForEach, Symbol};

    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"each_iv_mutate.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let a = mrb.intern_cstr(c"@a");
    let b = mrb.intern_cstr(c"@b");
    let added = mrb.intern_cstr(c"@added");
    obj.iv_set(&mrb, a, 1i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, b, 2i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");

    // The closure adds a variable and reassigns @b on every visit:
    // the mutations land on the receiver, while the iteration keeps
    // visiting the two variables and the values captured when it
    // began.
    let mut seen = Vec::new();
    obj.each_iv(&mrb, |name: Symbol, val| {
        obj.iv_set(&mrb, added, 9i32.into_value(&mrb))
            .expect("adding a variable mid-iteration lands on the receiver");
        obj.iv_set(&mrb, b, 99i32.into_value(&mrb))
            .expect("reassigning a variable mid-iteration lands on the receiver");
        seen.push((
            name.name(&mrb).expect("an ivar name interns to a name"),
            i32::from_value(val).expect("the seeded values are integers"),
        ));
        ForEach::Continue
    });
    seen.sort();

    assert_eq!(seen, vec![("@a".to_owned(), 1), ("@b".to_owned(), 2)]);
    assert_eq!(i32::from_value(obj.iv_get(&mrb, added)), Some(9));
    assert_eq!(i32::from_value(obj.iv_get(&mrb, b)), Some(99));
}

#[test]
fn each_iv_keeps_snapshot_values_alive_across_removal_and_gc() {
    use beni::{ForEach, RString};

    let mrb = open_mrb();
    let cxt = Ccontext::new(&mrb, c"each_iv_gc.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    let a = mrb.intern_cstr(c"@a");
    let b = mrb.intern_cstr(c"@b");

    // Release the strings' creation-time arena slots so the
    // receiver's iv table is their only reference going into the
    // iteration.
    let scope = mrb.arena_scope();
    obj.iv_set(&mrb, a, mrb.str_new(b"one").as_value())
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, b, mrb.str_new(b"two").as_value())
        .expect("iv_set on a fresh object does not raise");
    drop(scope);

    // The first visit removes every variable and runs a full
    // collection; the remaining snapshot value must still read
    // intact — the iteration owns its arena protection.
    let mut seen = Vec::new();
    let mut first = true;
    obj.each_iv(&mrb, |_, val| {
        if first {
            first = false;
            obj.iv_remove(&mrb, a).expect("removal does not raise");
            obj.iv_remove(&mrb, b).expect("removal does not raise");
            mrb.full_gc();
        }
        let s = RString::from_value(val).expect("the seeded values are strings");
        seen.push(String::from_utf8(s.to_bytes()).expect("the seeded bytes are UTF-8"));
        ForEach::Continue
    });
    seen.sort();

    assert_eq!(seen, vec!["one".to_owned(), "two".to_owned()]);
    assert!(!obj.iv_defined(&mrb, a), "the removals landed");
    assert!(!obj.iv_defined(&mrb, b), "the removals landed");
}

#[test]
fn each_iv_resurfaces_a_closure_panic_on_the_rust_side() {
    let mrb = open_mrb();
    let cxt =
        Ccontext::new(&mrb, c"each_iv_panic.rb").expect("allocating the context must succeed");
    let obj = cxt
        .load_nstring(b"Object.new")
        .expect("the test source must compile and run");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@a"), 1i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");
    obj.iv_set(&mrb, mrb.intern_cstr(c"@b"), 2i32.into_value(&mrb))
        .expect("iv_set on a fresh object does not raise");

    // A panic in the closure ends the iteration and propagates on the
    // Rust side — the closure runs against the collected snapshot, so
    // no mruby C frame is on the stack to unwind through. catch_unwind
    // sees the panic with its payload intact.
    let visited = std::cell::Cell::new(0u32);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        obj.each_iv(&mrb, |_, _| {
            visited.set(visited.get() + 1);
            panic!("boom in each_iv closure");
        });
    }));

    let payload = result.expect_err("the closure panic must resurface Rust-side");
    let msg = payload
        .downcast_ref::<&str>()
        .copied()
        .expect("the original panic payload survives the round-trip");
    assert_eq!(msg, "boom in each_iv closure");
    // The walk stopped at the first variable rather than running on.
    assert_eq!(visited.get(), 1);

    // The VM survives the caught panic.
    assert_eq!(
        i32::from_value(obj.iv_get(&mrb, mrb.intern_cstr(c"@b"))),
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
