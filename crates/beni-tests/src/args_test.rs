use beni::format::{Io, Kw, NRest, NRestKwBlock, Rest, RestBlock, Str, S};
use beni::{Mrb, Value};

/// Registered through `beni::method!(rest_count, -1)`: reads the rest
/// array via the `"*"` format and returns its length as an mruby
/// Integer.
fn rest_count(mrb: &Mrb, _self: Value) -> Value {
    let args = mrb.get_args::<Rest>();
    Value::from_int(mrb, args.len() as beni::sys::mrb_int)
}

/// Registered through `beni::method!(io_first, -1)`: reads the leading
/// `"i"` integer and the trailing `"o"` object, returning the
/// integer only when the object slot survived as `99` — so a read
/// that overruns the integer slot into the adjacent object fails
/// the assertion instead of passing.
fn io_first(mrb: &Mrb, _self: Value) -> Value {
    use beni::FromValue;
    let (n, val) = mrb.get_args::<Io>();
    if i32::from_value(val) == Some(99) {
        Value::from_int(mrb, n)
    } else {
        Value::from_int(mrb, -1)
    }
}

/// Registered through `beni::method!(nrest_after_sym, -1)`: reads the
/// `"n"` leading symbol and the `"*"` rest array, returning the
/// rest length only when the symbol decoded as `:tag` — so a read
/// that folds the symbol into the rest array (or shifts the count)
/// fails the assertion instead of passing.
fn nrest_after_sym(mrb: &Mrb, _self: Value) -> Value {
    let (sym, rest) = mrb.get_args::<NRest>();
    if sym == mrb.intern_cstr(c"tag") {
        Value::from_int(mrb, rest.len() as beni::sys::mrb_int)
    } else {
        Value::from_int(mrb, -1)
    }
}

// The `"*"` count out-param is written by mruby through `mrb_int*`
// (`GET_ARG(mrb_int*)` in vendor/mruby/src/class.c). Typing it
// narrower compiles under MRB_INT32 but corrupts the stack under
// 64-bit mrb_int — a width coincidence the repo's validation
// config cannot see. Exercising the full bridge → get_args →
// count path under whatever ABI the linked archive uses keeps
// that coincidence from coming back (`rake rust:test:default`
// runs this against an upstream-default 64-bit-mrb_int archive).
#[test]
fn rest_format_reads_the_argc_mruby_writes() {
    use beni::Module;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"rest_count", beni::method!(rest_count, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        Value::from_int(&mrb, 1),
        Value::from_int(&mrb, 2),
        Value::from_int(&mrb, 3),
    ];
    let count = receiver
        .funcall(&mrb, c"rest_count", &args)
        .expect("the bridge must not raise");

    assert!(count.is_integer(), "bridge must return an Integer");
    assert_eq!(unsafe { count.unbox_integer() }, 3);
}

// The `"i"` out-param is written by mruby as an `mrb_int` (8 bytes
// under the default 64-bit-mrb_int archive). A narrower out-param
// would write past its slot into the adjacent object value — the
// same width coincidence the rest test guards, reached through
// `"i"` rather than the `"*"` count. Asserting both the integer and
// the trailing object survive under `rake rust:test:default` keeps
// the `beni::sys::mrb_int` out-param honest.
#[test]
fn io_format_reads_the_int_mruby_writes() {
    use beni::Module;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"io_first", beni::method!(io_first, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [Value::from_int(&mrb, 7), Value::from_int(&mrb, 99)];
    let got = receiver
        .funcall(&mrb, c"io_first", &args)
        .expect("the bridge must not raise");

    assert!(got.is_integer(), "bridge must return an Integer");
    assert_eq!(
        unsafe { got.unbox_integer() },
        7,
        "a -1 means the trailing object slot did not survive the `\"i\"` read"
    );
}

// The `"n*"` read splits a leading symbol off before the rest
// array. The symbol argument is supplied from Ruby — the typed
// surface has no symbol-value constructor — so the call is driven
// through a compiled fragment, as the break test does.
#[test]
fn nrest_format_splits_the_leading_symbol() {
    use beni::{Ccontext, FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"nrest_after_sym", beni::method!(nrest_after_sym, -1))
        .expect("registering the bridge must succeed");

    let recv = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_nrest_recv");
    mrb.gv_set(slot, recv);

    let cxt =
        Ccontext::new(&mrb, c"nrest_test.rb").expect("allocating the compile context must succeed");
    let got = cxt.load_nstring(b"$beni_nrest_recv.nrest_after_sym(:tag, 1, 2, 3)");

    assert!(
        mrb.pending_exc().is_nil(),
        "the n* read must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(got), Some(3));
}

/// Registered through `beni::method!(s_echo, -1)`: reads the `"S"` String
/// argument and returns it unchanged.
fn s_echo(mrb: &Mrb, _self: Value) -> Value {
    mrb.get_args::<S>()
}

/// Registered through `beni::method!(str_echo, -1)`: reads the `"s"`
/// byte slice and copies it back into a fresh String, so the test
/// verifies both the pointer and the length survived the read.
fn str_echo(mrb: &Mrb, _self: Value) -> Value {
    let bytes = mrb.get_args::<Str>();
    mrb.str_new(bytes).as_value()
}

/// Registered through `beni::method!(rest_block_report, -1)`: reads the
/// `"*&"` rest array and block slot, returning the rest length when
/// a block was given and `-1` otherwise — so a read that folds the
/// block into the rest (or misplaces it) fails the assertion.
fn rest_block_report(mrb: &Mrb, _self: Value) -> Value {
    let (rest, block) = mrb.get_args::<RestBlock>();
    if block.is_nil() {
        Value::from_int(mrb, -1)
    } else {
        Value::from_int(mrb, rest.len() as beni::sys::mrb_int)
    }
}

#[test]
fn s_format_reads_a_string_argument() {
    use beni::Module;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"s_echo", beni::method!(s_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"s_echo", &[mrb.str_new(b"hello").as_value()])
        .expect("the bridge must not raise");

    assert!(got.is_string(), "the `\"S\"` read yields a String value");
    assert_eq!(got.to_string(&mrb), "hello");
}

#[test]
fn str_format_reads_a_string_as_bytes() {
    use beni::Module;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"str_echo", beni::method!(str_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"str_echo", &[mrb.str_new(b"hello").as_value()])
        .expect("the bridge must not raise");

    // The echoed String equals the input only if both the byte
    // pointer and the length were read correctly.
    assert_eq!(got.to_string(&mrb), "hello");
}

#[test]
fn rest_block_format_splits_rest_from_block() {
    use beni::{Ccontext, FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(
            &mrb,
            c"rest_block_report",
            beni::method!(rest_block_report, -1),
        )
        .expect("registering the bridge must succeed");

    let recv = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_rest_block_recv");
    mrb.gv_set(slot, recv);

    let cxt = Ccontext::new(&mrb, c"rest_block_test.rb")
        .expect("allocating the compile context must succeed");

    // A block is given: the three positionals land in the rest
    // array, the block in its own slot — rest length 3.
    let with_block = cxt.load_nstring(b"$beni_rest_block_recv.rest_block_report(1, 2, 3) { }");
    assert!(
        mrb.pending_exc().is_nil(),
        "the *& read must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(with_block), Some(3));

    // No block: the slot decodes as nil.
    let without_block = cxt.load_nstring(b"$beni_rest_block_recv.rest_block_report(1, 2)");
    assert_eq!(i32::from_value(without_block), Some(-1));
}

/// Registered through `beni::method!(arg1_echo, -1)`: reads the single
/// required argument via `Mrb::arg1` and returns it unchanged.
fn arg1_echo(mrb: &Mrb, _self: Value) -> Value {
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
    Value::from_int(mrb, mrb.argc())
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
    Value::from_int(mrb, sum)
}

#[test]
fn arg1_reads_the_single_argument() {
    use beni::{FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"arg1_echo", beni::method!(arg1_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let got = receiver
        .funcall(&mrb, c"arg1_echo", &[Value::from_int(&mrb, 42)])
        .expect("the single-argument read must not raise");
    assert_eq!(i32::from_value(got), Some(42));
}

#[test]
fn arg1_raises_argument_error_on_wrong_count() {
    use beni::{Error, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"arg1_echo", beni::method!(arg1_echo, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    // Two positionals: `mrb_get_arg1` raises ArgumentError rather
    // than returning the first — the strict-count contract.
    let args = [Value::from_int(&mrb, 1), Value::from_int(&mrb, 2)];
    let err = receiver
        .funcall(&mrb, c"arg1_echo", &args)
        .expect_err("a non-single argument count must surface as Err");
    match err {
        Error::Exception(exc) => assert_eq!(exc.classname(&mrb), "ArgumentError"),
        Error::Panic(_) => panic!("a wrong argument count must raise, not panic"),
    }
}

#[test]
fn argc_reads_the_argument_count() {
    use beni::{FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"argc_report", beni::method!(argc_report, -1))
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        Value::from_int(&mrb, 1),
        Value::from_int(&mrb, 2),
        Value::from_int(&mrb, 3),
    ];
    let got = receiver
        .funcall(&mrb, c"argc_report", &args)
        .expect("the count read must not raise");
    assert_eq!(i32::from_value(got), Some(3));
}

#[test]
fn argv_reads_the_whole_argument_array() {
    use beni::{FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
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
        Value::from_int(&mrb, 10),
        Value::from_int(&mrb, 20),
        Value::from_int(&mrb, 30),
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

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"block_report", beni::method!(block_report, -1))
        .expect("registering the bridge must succeed");

    let recv = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_block_recv");
    mrb.gv_set(slot, recv);

    // A block is supplied from Ruby — the typed surface has no
    // block-value constructor — so the call is driven through a
    // compiled fragment, as the *& read test does.
    let cxt = Ccontext::new(&mrb, c"block_given_test.rb")
        .expect("allocating the compile context must succeed");

    // No block: the predicate reports false.
    let without = cxt.load_nstring(b"$beni_block_recv.block_report");
    assert!(
        mrb.pending_exc().is_nil(),
        "the predicate must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(bool::from_value(without), Some(false));

    // A block is given: the predicate reports true.
    let with = cxt.load_nstring(b"$beni_block_recv.block_report { }");
    assert_eq!(bool::from_value(with), Some(true));
}

/// Registered through `beni::method!(rest_borrowed_survives_reentry, -1)`:
/// reads the `"*"` rest array as a borrowed slice, then re-enters the
/// VM while holding it — compiling and running a fragment that
/// allocates thousands of objects and recurses 400 frames deep — and
/// only then joins the elements. Pins the rest-form contract that the
/// borrow stays valid across VM re-entry: mruby backs it with a
/// GC-arena-rooted copy, so a read that had dangled would surface as a
/// corrupted join rather than the original bytes.
fn rest_borrowed_survives_reentry(mrb: &Mrb, _self: Value) -> Value {
    let rest = mrb.get_args::<Rest>();
    // Re-enter the VM while holding the borrow: compilation allocates
    // heavily and the recursion grows the value stack. The rest borrow
    // survives because it views an arena-backed copy, not the live stack.
    let cxt = beni::Ccontext::new(mrb, c"reentry_probe.rb").expect("compile context");
    cxt.load_nstring(
        b"def __probe_deep(n); return 0 if n <= 0; Array.new(16){ 'y' * 40 }; __probe_deep(n - 1); end; __probe_deep(400)",
    );
    // The probe must actually run: a swallowed compile or runtime error
    // would leave the value stack unstressed, letting a dangling read slip
    // through as a false pass rather than exercising the re-entry contract.
    assert!(
        mrb.pending_exc().is_nil(),
        "the re-entry probe must run cleanly: {}",
        mrb.pending_exc().to_string(mrb)
    );
    mrb.full_gc();
    let mut joined = String::new();
    for v in rest {
        joined.push_str(&v.to_string(mrb));
    }
    mrb.str_new(joined.as_bytes()).as_value()
}

#[test]
fn rest_borrowed_slice_survives_vm_reentry() {
    use beni::Module;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(
            &mrb,
            c"rest_borrowed_survives_reentry",
            beni::method!(rest_borrowed_survives_reentry, -1),
        )
        .expect("registering the bridge must succeed");

    let receiver = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let args = [
        mrb.str_new(b"al").as_value(),
        mrb.str_new(b"pha").as_value(),
    ];
    let got = receiver
        .funcall(&mrb, c"rest_borrowed_survives_reentry", &args)
        .expect("the borrowed read must not raise");

    assert_eq!(got.to_string(&mrb), "alpha");
}

/// Registered through `beni::method!(kw_size, -1)`: reads the `":"` keyword
/// bucket and returns its size. A nil bucket could not answer `size`,
/// so a clean `0` proves the empty case is an empty Hash, not nil.
fn kw_size(mrb: &Mrb, _self: Value) -> Value {
    let kw = mrb.get_args::<Kw>();
    Value::from_int(mrb, kw.len(mrb) as beni::sys::mrb_int)
}

/// Registered through `beni::method!(nrest_kwblock_encode, -1)`: reads the
/// `"n*:&"` shape and encodes the split as
/// `rest.len()*100 + kwargs.len()*10 + block`, returning `-1` unless
/// the leading symbol decoded as `:tag` — so a read that folds the
/// keywords into the rest, drops the symbol, or misplaces the block
/// fails the assertion instead of passing.
fn nrest_kwblock_encode(mrb: &Mrb, _self: Value) -> Value {
    let (sym, rest, kw, block) = mrb.get_args::<NRestKwBlock>();
    if sym != mrb.intern_cstr(c"tag") {
        return Value::from_int(mrb, -1);
    }
    let block_bit = if block.is_nil() { 0 } else { 1 };
    let code =
        rest.len() as beni::sys::mrb_int * 100 + kw.len(mrb) as beni::sys::mrb_int * 10 + block_bit;
    Value::from_int(mrb, code)
}

#[test]
fn kw_format_captures_keywords_and_empty_is_a_hash() {
    use beni::{Ccontext, FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(&mrb, c"kw_size", beni::method!(kw_size, -1))
        .expect("registering the bridge must succeed");

    let recv = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_kw_recv");
    mrb.gv_set(slot, recv);

    let cxt =
        Ccontext::new(&mrb, c"kw_test.rb").expect("allocating the compile context must succeed");

    // Two keywords land in the bucket.
    let two = cxt.load_nstring(b"$beni_kw_recv.kw_size(a: 1, b: 2)");
    assert!(
        mrb.pending_exc().is_nil(),
        "the : read must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(two), Some(2));

    // No keywords: the bucket is an empty Hash, not nil, so `size`
    // answers 0 rather than raising on a nil receiver.
    let none = cxt.load_nstring(b"$beni_kw_recv.kw_size");
    assert_eq!(i32::from_value(none), Some(0));
}

#[test]
fn nrest_kwblock_separates_positionals_keywords_and_block() {
    use beni::{Ccontext, FromValue, Module};

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let class = mrb.object_class();
    class
        .define_method(
            &mrb,
            c"nrest_kwblock_encode",
            beni::method!(nrest_kwblock_encode, -1),
        )
        .expect("registering the bridge must succeed");

    let recv = class
        .obj_new(&mrb, &[])
        .expect("the receiver constructs without raising");
    let slot = mrb.intern_cstr(c"$beni_kwblock_recv");
    mrb.gv_set(slot, recv);

    let cxt = Ccontext::new(&mrb, c"kwblock_test.rb")
        .expect("allocating the compile context must succeed");

    // A brace-less keyword stays in its own bucket: rest [1], kwargs
    // {a: 1}, no block -> 1*100 + 1*10 + 0.
    let kw = cxt.load_nstring(b"$beni_kwblock_recv.nrest_kwblock_encode(:tag, 1, a: 1)");
    assert!(
        mrb.pending_exc().is_nil(),
        "the n*:& read must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );
    assert_eq!(i32::from_value(kw), Some(110));

    // An explicit positional Hash stays among the positionals: rest
    // [1, {a: 1}], kwargs {} -> 2*100.
    let explicit = cxt.load_nstring(b"$beni_kwblock_recv.nrest_kwblock_encode(:tag, 1, {a: 1})");
    assert_eq!(i32::from_value(explicit), Some(200));

    // A block fills its own slot: rest [1], kwargs {a: 1}, block -> 111.
    let with_block =
        cxt.load_nstring(b"$beni_kwblock_recv.nrest_kwblock_encode(:tag, 1, a: 1) { }");
    assert_eq!(i32::from_value(with_block), Some(111));

    // No positionals or keywords: kwargs is an empty Hash, not nil, so
    // the encode reaches 0 only because `size` answered on a real Hash.
    let empty = cxt.load_nstring(b"$beni_kwblock_recv.nrest_kwblock_encode(:tag)");
    assert_eq!(i32::from_value(empty), Some(0));
}
