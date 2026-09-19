use crate::support::open_mrb;
use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::{Array, Error, IntoValue, Mrb, Value};

/// Registered through `beni::method!(rest_count, -1)`: reads the splat
/// and returns its length as an mruby Integer.
fn rest_count(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let splat = scan_args::<(), (), Array, (), (), ()>(mrb)?.splat;
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
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        1i32.into_value(&mrb),
        2i32.into_value(&mrb),
        3i32.into_value(&mrb),
    ];
    let count = receiver
        .funcall(&mrb, c"rest_count", &args)
        .expect("the bridge must not raise");

    assert!(count.is_integer(), "bridge must return an Integer");
    assert_eq!(unsafe { count.unbox_integer() }, 3);
}

/// Registered through `beni::method!(arg1_echo, -1)`: reads the single
/// required argument via `Mrb::arg1` and returns it unchanged.
fn arg1_echo(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    mrb.arg1()
}

/// Registered through `beni::method!(block_report, -1)`: returns whether a
/// block was passed to the call as a Ruby boolean, read via
/// `Mrb::block_given`.
fn block_report(mrb: &Mrb, _self: Value) -> Value {
    use beni::IntoValue;
    mrb.block_given().into_value(mrb)
}

/// Registered through `beni::method!(argc_report, -1)`: returns the
/// argument count read via `Mrb::argc` as an mruby Integer.
fn argc_report(mrb: &Mrb, _self: Value) -> Value {
    (mrb.argc() as i32).into_value(mrb)
}

/// Registered through `beni::method!(argv_sum, -1)`: reads the whole
/// positional argument array via `Mrb::argv` and returns the sum of
/// the arguments, decoded as integers. Summing every slot — not just
/// the first — fails the assertion if the slice length or any element
/// is wrong, and the no-argument call exercises the empty-slice path
/// (sum 0).
fn argv_sum(mrb: &Mrb, _self: Value) -> Value {
    use beni::FromValue;
    let sum: beni::sys::mrb_int = mrb
        .argv()
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
        .obj_new(&mrb, &[])
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
        .obj_new(&mrb, &[])
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
fn argc_reads_the_argument_count() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"argc_report", beni::method!(argc_report, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        1i32.into_value(&mrb),
        2i32.into_value(&mrb),
        3i32.into_value(&mrb),
    ];
    let got = receiver
        .funcall(&mrb, c"argc_report", &args)
        .expect("the count read must not raise");
    assert_eq!(i32::from_value(got), Some(3));
}

#[test]
fn argv_reads_the_whole_argument_array() {
    use beni::{FromValue, Module};

    let mrb = open_mrb();
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"argv_sum", beni::method!(argv_sum, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");

    // Several arguments: the body reads every slot and sums them, so
    // a short or misread slice would not total 60.
    let args = [
        10i32.into_value(&mrb),
        20i32.into_value(&mrb),
        30i32.into_value(&mrb),
    ];
    let got = receiver
        .funcall(&mrb, c"argv_sum", &args)
        .expect("the array read must not raise");
    assert_eq!(i32::from_value(got), Some(60));

    // No arguments: `argv` yields an empty slice, summed to 0,
    // exercising the `argc == 0` path without forming a slice from
    // the call frame pointer.
    let empty = receiver
        .funcall(&mrb, c"argv_sum", &[])
        .expect("the empty array read must not raise");
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
        .obj_new(&mrb, &[])
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

fn guarded_arg1(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
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

    let err = Value::nil()
        .funcall(&mrb, c"read_arg1", &[one, one])
        .expect_err("a call the read does not fit must reach the caller as a raise");

    let Error::Exception(exc) = err else {
        panic!("the read must raise an exception, got {err}");
    };
    assert_eq!(exc.classname(&mrb), "ArgumentError");
    assert_eq!(GUARD_DROPS.with(core::cell::Cell::get), before + 1);
}

// Holds the argument-array read across a re-entry that grows the value
// stack, then joins what it held.
fn argv_survives_reentry(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args = mrb.argv();
    let cxt = beni::Ccontext::new(mrb, c"reentry_probe.rb").expect("compile context");
    cxt.load_nstring(
        b"def __probe_deep(n); return 0 if n <= 0; Array.new(16){ 'y' * 40 }; __probe_deep(n - 1); end; __probe_deep(400)",
    )
    .expect("the test source must compile and run");
    mrb.full_gc();
    let mut joined = String::new();
    for v in &args {
        joined.push_str(&v.to_string(mrb));
    }
    Ok(mrb.str_new(joined.as_bytes()).as_value())
}

#[test]
fn argv_copy_survives_vm_reentry() {
    use beni::Module;

    let mrb = open_mrb();
    mrb.object_class()
        .define_method(
            &mrb,
            c"argv_survives_reentry",
            beni::method!(argv_survives_reentry, -1),
        )
        .expect("registering the bridge must succeed");
    let args = [
        mrb.str_new(b"al").as_value(),
        mrb.str_new(b"pha").as_value(),
    ];

    let got = Value::nil()
        .funcall(&mrb, c"argv_survives_reentry", &args)
        .expect("the read must not raise");

    assert_eq!(got.to_string(&mrb), "alpha");
}
