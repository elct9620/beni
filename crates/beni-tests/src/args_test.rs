use crate::support::{hashes_on_the_heap, open_mrb, Is};
use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::{Error, FromValue, IntoValue, Mrb, RArray, Value};

/// Registered through `beni::method!(rest_count, -1)`: reads the splat
/// and returns its length as an mruby Integer.
fn rest_count(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let splat = scan_args::<(), (), RArray, (), (), ()>(mrb)?.splat;
    Ok((splat.len() as i32).into_value(mrb))
}

// The `"*"` count out-param is written by mruby through `mrb_int*`
// (`GET_ARG(mrb_int*)` in vendor/mruby/src/class.c). Typing it
// narrower compiles under MRB_INT32 but corrupts the stack under
// 64-bit mrb_int — a width coincidence the repo's validation
// config cannot see. Exercising the full bridge → scan_args →
// count path under whatever ABI the linked archive uses keeps
// that coincidence from coming back (`rake rust:test:default`
// runs this against an upstream-default 64-bit-mrb_int archive).
#[test]
fn a_splat_reads_the_argc_mruby_writes() {
    use beni::Module;

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"rest_count", beni::method!(rest_count, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        1i32.into_value(&mrb),
        2i32.into_value(&mrb),
        3i32.into_value(&mrb),
    ];
    let count = receiver
        .funcall(&mrb, c"rest_count", &args)
        .expect("the bridge must not raise");

    assert!(count.is::<beni::Integer>(), "bridge must return an Integer");
    assert_eq!(i64::from_value(count).expect("an Integer"), 3);
}

/// Registered through `beni::method!(arg1_echo, -1)`: reads the single
/// required argument via `Mrb::arg1` and returns it unchanged.
fn arg1_echo(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    mrb.arg1()
}

/// Registered through `beni::method!(block_report, -1)`: returns whether a
/// block was passed to the call as a Ruby boolean, read via
/// `Mrb::block_given`.
fn block_report(mrb: &Mrb, _self: Value, _args: &[Value]) -> Value {
    use beni::IntoValue;
    mrb.block_given().into_value(mrb)
}

/// Registered through `beni::method!(args_len, -1)`: returns the length
/// of the argument slice as an mruby Integer.
fn args_len(mrb: &Mrb, _self: Value, args: &[Value]) -> Value {
    (args.len() as i32).into_value(mrb)
}

/// Registered through `beni::method!(args_sum, -1)`: returns the sum of
/// the argument slice, decoded as integers. Summing every slot — not just
/// the first — fails the assertion if the slice length or any element
/// is wrong, and the no-argument call exercises the empty slice (sum 0).
fn args_sum(mrb: &Mrb, _self: Value, args: &[Value]) -> Value {
    use beni::FromValue;
    let sum: beni::sys::mrb_int = args
        .iter()
        .map(|v| beni::sys::mrb_int::from(i32::from_value(*v).unwrap_or(0)))
        .sum();
    sum.into_value(mrb)
}

#[test]
fn arg1_reads_the_single_argument() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"arg1_echo", beni::method!(arg1_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"arg1_echo", &[42i32.into_value(&mrb)])
        .expect("the single-argument read must not raise");
    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn arg1_raises_argument_error_on_wrong_count() {
    use beni::{Error, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"arg1_echo", beni::method!(arg1_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");
    // Two positionals: `mrb_get_arg1` raises ArgumentError rather
    // than returning the first — the strict-count contract.
    let args = [1i32.into_value(&mrb), 2i32.into_value(&mrb)];
    let err = receiver
        .funcall(&mrb, c"arg1_echo", &args)
        .expect_err("a non-single argument count must surface as Err");
    match err {
        Error::Exception(exc) => assert_eq!(exc.classname(&mrb), "ArgumentError"),
        other => panic!("a wrong argument count must raise, not panic, got {other}"),
    }
}

#[test]
fn an_any_arity_method_receives_every_argument() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"args_sum", beni::method!(args_sum, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");

    // Several arguments: the body reads every slot and sums them, so
    // a short or misread slice would not total 60.
    let args = [
        10i32.into_value(&mrb),
        20i32.into_value(&mrb),
        30i32.into_value(&mrb),
    ];
    let got = receiver
        .funcall(&mrb, c"args_sum", &args)
        .expect("the call must not raise");
    assert_eq!(i32::from_value(got), Some(60));

    // No arguments: the slice is empty, summed to 0, without forming a
    // slice from the call frame pointer.
    let empty = receiver
        .funcall(&mrb, c"args_sum", &[])
        .expect("the empty call must not raise");
    assert_eq!(i32::from_value(empty), Some(0));
}

#[test]
fn block_given_reports_whether_a_block_was_passed() {
    use beni::{Ccontext, FromValue, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"block_report", beni::method!(block_report, -1))
        .expect("registering the bridge must succeed");

    let recv = class
        .new_instance(&mrb, &[])
        .expect("the receiver constructs without raising");
    mrb.gv_set(c"$beni_block_recv", recv)
        .expect("the name interns");

    // A block is supplied from Ruby — the typed surface has no
    // block-value constructor — so the call is driven through a
    // compiled fragment, as the *& read test does.
    let cxt = Ccontext::new(&mrb, c"block_given_test.rb")
        .expect("allocating the compile context must succeed");

    // No block: the predicate reports false.
    let without = cxt
        .load_nstring(b"$beni_block_recv.block_report")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "the predicate must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(bool::from_value(without), Some(false));

    // A block is given: the predicate reports true.
    let with = cxt
        .load_nstring(b"$beni_block_recv.block_report { }")
        .expect("the test source must compile and run");
    assert_eq!(bool::from_value(with), Some(true));
}

thread_local! {
    static GUARD_DROPS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

// Held by a reader body across its read, so a test can see whether the
// body was left through its own return rather than jumped over.
struct ReadGuard;

impl Drop for ReadGuard {
    fn drop(&mut self) {
        GUARD_DROPS.with(|drops| drops.set(drops.get() + 1));
    }
}

fn guarded_arg1(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let _guard = ReadGuard;
    mrb.arg1()
}

#[test]
fn a_call_the_single_argument_read_does_not_fit_leaves_the_body_through_its_own_return() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"read_arg1", beni::method!(guarded_arg1, -1))
        .expect("registering the reader must succeed");
    let one = 1i32.into_value(&mrb);
    let before = GUARD_DROPS.with(core::cell::Cell::get);

    let err = beni::value::qnil()
        .as_value()
        .funcall(&mrb, c"read_arg1", &[one, one])
        .expect_err("a call the read does not fit must reach the caller as a raise");

    let Error::Exception(exc) = err else {
        panic!("the read must raise an exception, got {err}");
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
    assert_eq!(GUARD_DROPS.with(core::cell::Cell::get), before + 1);
}

// Holds the argument slice across a re-entry that grows the value stack,
// then joins what it held.
fn args_survive_reentry(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let cxt = beni::Ccontext::new(mrb, c"reentry_probe.rb").expect("compile context");
    cxt.load_nstring(
        b"def __probe_deep(n); return 0 if n <= 0; Array.new(16){ 'y' * 40 }; __probe_deep(n - 1); end; __probe_deep(400)",
    )
    .expect("the test source must compile and run");
    mrb.full_gc();
    let mut joined = String::new();
    for v in args {
        joined.push_str(&v.to_string(mrb));
    }
    Ok(mrb.str_new(joined.as_bytes()).as_value())
}

#[test]
fn the_argument_slice_survives_vm_reentry() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(
            &mrb,
            c"args_survive_reentry",
            beni::method!(args_survive_reentry, -1),
        )
        .expect("registering the bridge must succeed");
    let args = [
        mrb.str_new(b"al").as_value(),
        mrb.str_new(b"pha").as_value(),
    ];

    let got = beni::value::qnil()
        .as_value()
        .funcall(&mrb, c"args_survive_reentry", &args)
        .expect("the call must not raise");

    assert_eq!(got.to_string(&mrb), "alpha");
}

fn eval(mrb: &Mrb, source: &str) -> Value {
    mrb.load_string(source.as_bytes())
        .unwrap_or_else(|err| panic!("{source} raised: {}", err.message(mrb)))
}

#[test]
fn the_slice_holds_a_non_empty_keyword_hash_as_one_trailing_value() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"args_len", beni::method!(args_len, -1))
        .expect("registering the bridge must succeed");
    let len = |source: &str| i32::from_value(eval(&mrb, source));

    assert_eq!(len("args_len(1, a: 2)"), Some(2));
    assert_eq!(len("args_len(1, {a: 2})"), Some(2));
    assert_eq!(len("args_len(1, **{})"), Some(1));
    // Below, at, and past the inline copy, and past the fifteen
    // positionals mruby packs into one array, the keyword hash still
    // counts once.
    assert_eq!(len("args_len(*(1..7), a: 1)"), Some(8));
    assert_eq!(len("args_len(*(1..8), a: 1)"), Some(9));
    assert_eq!(len("args_len(*(1..13), a: 1)"), Some(14));
    assert_eq!(len("args_len(*(1..14), a: 1)"), Some(15));
    assert_eq!(len("args_len(*(1..15), a: 1)"), Some(16));
}

// The argument slice rendered, so its order and last value show.
fn args_inspect(mrb: &Mrb, _self: Value, args: &[Value]) -> Value {
    mrb.ary_new_from_values(args).as_value()
}

#[test]
fn the_argument_slice_ends_with_the_keyword_hash() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"args_inspect", beni::method!(args_inspect, -1))
        .expect("registering the bridge must succeed");

    assert_eq!(
        eval(&mrb, "args_inspect(1, a: 2)").inspect(&mrb),
        "[1, {a: 2}]"
    );
    assert_eq!(
        eval(&mrb, "args_inspect(*(1..15), a: 1)").inspect(&mrb),
        "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, {a: 1}]"
    );
}

#[test]
fn the_argument_slice_holds_every_positional_mruby_packs_into_one_array() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"args_inspect", beni::method!(args_inspect, -1))
        .expect("registering the bridge must succeed");

    assert_eq!(
        eval(&mrb, "[args_inspect(*(1..16)), args_inspect(1, 2)]").inspect(&mrb),
        "[[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], [1, 2]]"
    );
}

// Clears the keyword hash the slice ends with, then answers the size of
// the keyword bucket a scan read finds.
fn clear_then_scan(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let handed = beni::RHash::from_value(*args.last().expect("the call passed keywords"))
        .expect("the trailing value is the keyword hash");
    handed.clear(mrb)?;
    let bucket = scan_args::<(), (), RArray, (), beni::RHash, ()>(mrb)?.keywords;
    Ok((bucket.len(mrb) as i32).into_value(mrb))
}

#[test]
fn the_slice_carries_the_calls_own_keyword_hash() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(&mrb, c"clear_then_scan", beni::method!(clear_then_scan, -1))
        .expect("registering the bridge must succeed");

    let found = eval(&mrb, "clear_then_scan(a: 1, b: 2)");

    assert_eq!(i32::from_value(found), Some(0));
}

// [slice length, keyword bucket size] — the scan read runs after the
// slice was handed over.
fn keywords_after_the_slice(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let bucket = scan_args::<(), (), RArray, (), beni::RHash, ()>(mrb)?.keywords;
    Ok(mrb
        .ary_new_from_values(&[
            (args.len() as i32).into_value(mrb),
            (bucket.len(mrb) as i32).into_value(mrb),
        ])
        .as_value())
}

#[test]
fn a_scan_read_still_finds_the_keywords_after_the_slice() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(
            &mrb,
            c"keywords_after",
            beni::method!(keywords_after_the_slice, -1),
        )
        .expect("registering the bridge must succeed");

    assert_eq!(
        eval(&mrb, "keywords_after(1, a: 2)").inspect(&mrb),
        "[2, 1]"
    );
}

fn arg_count(_mrb: &Mrb, _self: Value, args: &[Value]) -> i32 {
    args.len() as i32
}

#[test]
fn an_any_arity_call_without_keywords_allocates_no_hash() {
    let mrb = open_mrb();
    mrb.object_class()
        .define_singleton_method(&mrb, "count", beni::method!(arg_count, -1))
        .unwrap();
    let calls = mrb
        .load_string(b"GC.disable; proc { 1000.times { Object.count(1, 2) } }")
        .unwrap();
    let calls = beni::Proc::from_value(calls).unwrap();
    let before = hashes_on_the_heap(&mrb);

    calls.call(&mrb, &[]).unwrap();

    assert_eq!(hashes_on_the_heap(&mrb), before);
}

// One function registered under several names, answering the name it
// was reached by, or nil when the read finds none.
fn called_as(mrb: &Mrb, _self: Value) -> Value {
    mrb.mid()
        .map_or(beni::value::qnil().as_value(), |id| id.into_value(mrb))
}

#[test]
fn a_method_reads_the_name_its_call_reached_it_by() {
    use beni::Module;
    let mrb = open_mrb();
    let class = mrb
        .define_class(c"BeniCalledAs", mrb.object_class())
        .expect("defining the class must succeed");
    class
        .define_method(&mrb, c"first", beni::method!(called_as, 0))
        .expect("registering first must succeed");
    class
        .define_method(&mrb, c"second", beni::method!(called_as, 0))
        .expect("registering second must succeed");

    let got = mrb
        .load_string(
            b"class BeniCalledAs; alias_method :third, :first; end
              o = BeniCalledAs.new
              [o.first, o.second, o.third, o.send(:second)] == [:first, :second, :third, :second]",
        )
        .expect("each call reads its own name");

    assert_eq!(bool::from_value(got), Some(true));
}

#[test]
fn the_name_read_answers_none_outside_any_method_call() {
    let mrb = open_mrb();

    assert!(
        mrb.mid().is_none(),
        "Rust code outside any call has no name"
    );
    mrb.load_string(b"1 + 1").expect("top-level code runs");
    assert!(
        mrb.mid().is_none(),
        "a finished top-level run leaves no name"
    );
}
