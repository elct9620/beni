use beni::{Ccontext, Error, FromValue, Mrb, RString};

#[test]
fn cat_appends_bytes_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"foo");
    s.cat(&mrb, b"bar")
        .expect("appending to a mutable string succeeds");
    // The same handle now names the grown string — append mutated it
    // in place rather than producing a new object.
    assert_eq!(s.to_bytes(), b"foobar".to_vec());

    // Appending empty bytes leaves the receiver unchanged.
    s.cat(&mrb, b"").expect("appending nothing succeeds");
    assert_eq!(s.to_bytes(), b"foobar".to_vec());
}

#[test]
fn cat_appends_a_static_literal_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A `b"..."` literal is a `&'static [u8]`, so `cat` reaches what
    // C's `mrb_str_cat_lit(mrb, str, lit)` does — the literal-append path.
    let s = mrb.str_new(b"foo");
    s.cat(&mrb, b"bar").expect("appending a literal succeeds");
    assert_eq!(s.to_bytes(), b"foobar".to_vec());
}

#[test]
fn cat_str_appends_another_string_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"foo");
    let tail = mrb.str_new(b"bar");
    s.cat_str(&mrb, tail)
        .expect("appending a string to a mutable string succeeds");
    // The receiver grew in place; the source is untouched.
    assert_eq!(s.to_bytes(), b"foobar".to_vec());
    assert_eq!(tail.to_bytes(), b"bar".to_vec());

    // Self-append doubles the receiver — the source snapshot is taken
    // before the buffer grows.
    s.cat_str(&mrb, s).expect("self-append succeeds");
    assert_eq!(s.to_bytes(), b"foobarfoobar".to_vec());
}

#[test]
fn cat_str_surfaces_frozen_receiver_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

    let frozen = RString::from_value(
        cxt.load_nstring(b"'fixed'.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen String literal is String-tagged");
    assert!(
        mrb.pending_exc().is_nil(),
        "freezing the string must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    let result = frozen.cat_str(&mrb, mrb.str_new(b"more"));
    assert!(matches!(result, Err(Error::Exception(_))));
}

#[test]
fn cat_cstr_appends_a_c_string_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"foo");
    s.cat_cstr(&mrb, c"bar")
        .expect("appending a C string to a mutable string succeeds");
    // The same handle now names the grown string — the bytes up to the
    // terminating NUL were appended in place.
    assert_eq!(s.to_bytes(), b"foobar".to_vec());

    // Appending an empty C string leaves the receiver unchanged.
    s.cat_cstr(&mrb, c"").expect("appending nothing succeeds");
    assert_eq!(s.to_bytes(), b"foobar".to_vec());
}

#[test]
fn cat_cstr_surfaces_frozen_receiver_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

    let frozen = RString::from_value(
        cxt.load_nstring(b"'fixed'.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen String literal is String-tagged");
    let result = frozen.cat_cstr(&mrb, c"more");
    assert!(matches!(result, Err(Error::Exception(_))));
}

#[test]
fn cat_surfaces_frozen_receiver_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

    // A frozen String still carries the String tag, so the downcast
    // holds, but appending to it raises FrozenError — which protect
    // catches into Err rather than long-jumping.
    let frozen = RString::from_value(
        cxt.load_nstring(b"'fixed'.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen String literal is String-tagged");
    assert!(
        mrb.pending_exc().is_nil(),
        "freezing the string must not raise: {}",
        mrb.pending_exc().to_string(&mrb)
    );

    let result = frozen.cat(&mrb, b"more");
    assert!(matches!(result, Err(Error::Exception(_))));
}

#[test]
fn len_and_is_empty_track_the_byte_count() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let empty = mrb.str_new(b"");
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    // The count is bytes, not characters: a 2-byte UTF-8 codepoint
    // contributes its bytes, so "héllo" measures 6, not 5.
    let s = mrb.str_new("héllo".as_bytes());
    assert_eq!(s.len(), 6);
    assert!(!s.is_empty());
}

#[test]
fn dup_copies_into_an_independent_string() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"orig");
    let copy = s.dup(&mrb);

    // dup is an independent object: appending to the original leaves
    // the copy untouched.
    s.cat(&mrb, b"+more").expect("append succeeds");
    assert_eq!(copy.to_bytes(), b"orig".to_vec());
    assert_eq!(s.to_bytes(), b"orig+more".to_vec());
}

#[test]
fn plus_concatenates_into_a_new_string_leaving_operands_unchanged() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let a = mrb.str_new(b"foo");
    let b = mrb.str_new(b"bar");
    // Capture both operands' bytes before the call to prove
    // non-mutation against the post-call reads.
    let a_before = a.to_bytes();
    let b_before = b.to_bytes();

    let joined = a.plus(&mrb, b);

    // The result is the concatenation of both operands.
    assert_eq!(joined.to_bytes(), b"foobar".to_vec());

    // Neither operand was mutated — plus builds a new string rather
    // than growing the receiver the way cat_str does.
    assert_eq!(a.to_bytes(), a_before);
    assert_eq!(b.to_bytes(), b_before);

    // The result is an independent object: growing it in place leaves
    // the receiver untouched.
    joined
        .cat(&mrb, b"!")
        .expect("appending to the result succeeds");
    assert_eq!(joined.to_bytes(), b"foobar!".to_vec());
    assert_eq!(a.to_bytes(), b"foo".to_vec());
}

#[test]
fn cmp_orders_by_byte_content() {
    use core::cmp::Ordering;

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let abc = mrb.str_new(b"abc");
    let abd = mrb.str_new(b"abd");
    let abc2 = mrb.str_new(b"abc");

    assert_eq!(abc.cmp(&mrb, abd), Ordering::Less);
    assert_eq!(abd.cmp(&mrb, abc), Ordering::Greater);
    assert_eq!(abc.cmp(&mrb, abc2), Ordering::Equal);

    // A prefix orders before the longer string it begins.
    let ab = mrb.str_new(b"ab");
    assert_eq!(ab.cmp(&mrb, abc), Ordering::Less);
}

#[test]
fn eq_tests_byte_equality() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let abc = mrb.str_new(b"abc");
    let abc2 = mrb.str_new(b"abc");
    let abd = mrb.str_new(b"abd");

    // Same bytes in distinct objects are equal.
    assert!(abc.eq(&mrb, abc2));

    // Differing bytes of equal length are unequal.
    assert!(!abc.eq(&mrb, abd));

    // A prefix is unequal to the longer string it begins — the length
    // check rejects it before the byte compare.
    let ab = mrb.str_new(b"ab");
    assert!(!ab.eq(&mrb, abc));
}

#[test]
fn intern_names_the_symbol_for_the_receiver_bytes() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // The interned symbol names the string's own bytes.
    let sym = mrb.str_new(b"flags").intern(&mrb);
    assert_eq!(sym.name(&mrb).as_deref(), Some("flags"));

    // Its id equals interning the same name directly — a wrong tag or
    // boxing in the unchecked wrap would diverge here.
    assert_eq!(sym.to_sym(), mrb.intern_cstr(c"flags"));
}

#[test]
fn to_bytes_copies_arbitrary_bytes() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // Binary bytes survive the owned copy — `to_bytes` does not
    // require valid UTF-8.
    let s = mrb.str_new(&[0xff, 0x00, 0xfe]);
    assert_eq!(s.to_bytes(), vec![0xff, 0x00, 0xfe]);
}

#[test]
fn concat_coerces_a_non_string_argument_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A plain String argument appends like cat_str.
    let s = mrb.str_new(b"foo");
    s.concat(&mrb, mrb.str_new(b"bar").as_value())
        .expect("appending a string succeeds");
    assert_eq!(s.to_bytes(), b"foobar".to_vec());

    // A non-string argument is coerced before appending: an Integer
    // renders to its decimal text.
    s.concat(&mrb, beni::Value::from_int(&mrb, 42))
        .expect("appending a coerced integer succeeds");
    assert_eq!(s.to_bytes(), b"foobar42".to_vec());
}

#[test]
fn concat_surfaces_frozen_receiver_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

    let frozen = RString::from_value(
        cxt.load_nstring(b"'fixed'.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen String literal is String-tagged");
    let result = frozen.concat(&mrb, mrb.str_new(b"more").as_value());
    assert!(matches!(result, Err(Error::Exception(_))));
}

#[test]
fn resize_truncates_and_extends_in_place() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // Shrinking drops the tail; the same handle names the result.
    let s = mrb.str_new(b"Hello, world!");
    s.resize(&mrb, 5).expect("shrinking succeeds");
    assert_eq!(s.to_bytes(), b"Hello".to_vec());

    // Growing extends the length; the original prefix is preserved,
    // the new tail's contents are unspecified, so only the length is
    // asserted.
    s.resize(&mrb, 8).expect("growing succeeds");
    assert_eq!(s.len(), 8);
    assert_eq!(&s.to_bytes()[..5], b"Hello");
}

#[test]
fn resize_surfaces_frozen_receiver_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"frozen_test.rb").expect("allocating the context must succeed");

    let frozen = RString::from_value(
        cxt.load_nstring(b"'fixed'.freeze")
            .expect("the test source must compile and run"),
    )
    .expect("a frozen String literal is String-tagged");
    assert!(matches!(frozen.resize(&mrb, 2), Err(Error::Exception(_))));
}

#[test]
fn to_cstr_yields_a_nul_terminated_view() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"hello");
    let cstr = s.to_cstr(&mrb).expect("a NUL-free string yields a CString");
    assert_eq!(cstr.to_bytes(), b"hello");
    // The view carries the terminating NUL the C boundary expects.
    assert_eq!(cstr.to_bytes_with_nul(), b"hello\0");
}

#[test]
fn to_cstr_surfaces_an_embedded_nul_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A C string cannot carry an embedded NUL, so the read raises
    // ArgumentError, which protect catches into Err.
    let s = mrb.str_new(b"a\0b");
    assert!(matches!(s.to_cstr(&mrb), Err(Error::Exception(_))));
}

#[test]
fn substr_reads_a_range_and_clamps_out_of_range() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"Hello, world!");

    // An in-range slice yields the substring.
    let he = s.substr(&mrb, 0, 2).expect("an in-range slice is Some");
    assert_eq!(he.to_bytes(), b"He".to_vec());

    // A negative beg counts from the end.
    let bang = s.substr(&mrb, -1, 1).expect("a tail slice is Some");
    assert_eq!(bang.to_bytes(), b"!".to_vec());

    // An over-long len clamps to the string's end.
    let tail = s.substr(&mrb, 7, 100).expect("an over-long len clamps");
    assert_eq!(tail.to_bytes(), b"world!".to_vec());

    // A beg past the end yields None, the way mruby returns nil.
    assert!(s.substr(&mrb, 100, 1).is_none());
}

#[test]
fn substr_saturates_an_out_of_width_beg_rather_than_wrapping() {
    // An out-of-width beg is only representable when the host `i64` is
    // wider than `mrb_int`, i.e. a 32-bit `mrb_int`. Under a 64-bit
    // `mrb_int` the saturation premise is vacuous and the case is
    // skipped rather than asserted on a width it cannot reach.
    if core::mem::size_of::<beni::sys::mrb_int>() != 4 {
        return;
    }

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"Hello");

    // 0x1_0000_0002 is out of an MRB_INT32 archive's `mrb_int` range; a
    // truncating `as` cast would wrap it to the in-range beg 2 and slice
    // "llo". Saturating to the upper bound keeps it "past the end", so
    // the read is None — the same as any genuinely out-of-range beg.
    assert!(s.substr(&mrb, 0x1_0000_0002, 1).is_none());

    // The negative counterpart: a truncating cast would wrap
    // -0x1_0000_0002 to the in-range beg -2 and slice "lo" off the
    // tail. Saturating to the lower bound keeps it before the
    // beginning, so the read is None.
    assert!(s.substr(&mrb, -0x1_0000_0002, 2).is_none());
}

#[test]
fn index_finds_the_first_match_at_or_after_the_offset() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"hello, hello");

    // A present substring returns the byte index of its first match.
    assert_eq!(s.index(&mrb, b"llo", 0), Some(2));

    // An absent substring returns None.
    assert!(s.index(&mrb, b"xyz", 0).is_none());

    // The offset is respected: a match before it is skipped, and the
    // next match's index is reported.
    assert_eq!(s.index(&mrb, b"llo", 3), Some(9));

    // A negative offset counts from the end.
    assert_eq!(s.index(&mrb, b"hello", -5), Some(7));

    // An empty needle is found at the offset itself.
    assert_eq!(s.index(&mrb, b"", 4), Some(4));

    // An offset past the end finds nothing.
    assert!(s.index(&mrb, b"hello", 100).is_none());
}

#[test]
fn index_saturates_an_out_of_width_offset_rather_than_wrapping() {
    // An out-of-width offset is only representable when the host `i64`
    // is wider than `mrb_int`, i.e. a 32-bit `mrb_int`. Under a 64-bit
    // `mrb_int` the saturation premise is vacuous and the case is
    // skipped rather than asserted on a width it cannot reach.
    if core::mem::size_of::<beni::sys::mrb_int>() != 4 {
        return;
    }

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let s = mrb.str_new(b"hello, hello");

    // 0x1_0000_0003 is out of an MRB_INT32 archive's `mrb_int` range; a
    // truncating `as` cast would wrap it to the in-range offset 3 and
    // report the second "hello" at byte 7. Saturating to the upper bound
    // keeps it "past the end", so nothing is found — the same as any
    // genuinely out-of-range offset.
    assert!(s.index(&mrb, b"hello", 0x1_0000_0003).is_none());

    // The negative counterpart: a truncating cast would wrap
    // -0x1_0000_0003 to the in-range offset -3 and find the tail
    // "llo"'s match. Saturating to the lower bound keeps it before
    // the beginning, so nothing is found.
    assert!(s.index(&mrb, b"llo", -0x1_0000_0003).is_none());
}

#[test]
fn to_i_parses_in_the_requested_base() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A clean decimal string parses to its value.
    assert_eq!(
        mrb.str_new(b"12345")
            .to_i(&mrb, 10)
            .expect("a decimal string parses"),
        12345
    );

    // The same digits read in base 16 take their hexadecimal value.
    assert_eq!(
        mrb.str_new(b"ff")
            .to_i(&mrb, 16)
            .expect("a hex string parses"),
        255
    );

    // Base 0 auto-detects a leading prefix.
    assert_eq!(
        mrb.str_new(b"0b101")
            .to_i(&mrb, 0)
            .expect("a prefixed string auto-detects its base"),
        5
    );
}

#[test]
fn to_i_surfaces_invalid_input_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // Trailing junk is rejected by the strict parse — unlike Ruby's
    // lenient String#to_i, which would stop at the first bad character.
    assert!(matches!(
        mrb.str_new(b"99 red balloons").to_i(&mrb, 10),
        Err(Error::Exception(_))
    ));

    // Bytes with no valid integer at all raise too.
    assert!(matches!(
        mrb.str_new(b"hello").to_i(&mrb, 10),
        Err(Error::Exception(_))
    ));
}

#[test]
fn to_i_aliases_a_negative_base_to_its_radix_without_raising() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A negative base is not an illegal radix: -16 aliases radix 16 with
    // prefix detection disabled, so the bytes parse in base 16 instead of
    // raising ArgumentError.
    assert_eq!(
        mrb.str_new(b"ff")
            .to_i(&mrb, -16)
            .expect("a negative base aliases its radix"),
        255
    );
}

#[test]
fn to_inum_parses_leniently_without_raising_on_malformed_content() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A clean decimal string parses to its value.
    assert_eq!(
        mrb.str_new(b"12345")
            .to_inum(&mrb, 10)
            .expect("a decimal string parses"),
        12345
    );

    // Trailing junk is ignored — unlike strict to_i, the lenient parse
    // consumes the leading integer and stops, the way Ruby's String#to_i
    // does.
    assert_eq!(
        mrb.str_new(b"12abc")
            .to_inum(&mrb, 10)
            .expect("a malformed-prefix string reads its leading integer"),
        12
    );

    // Bytes with no integer at the start yield 0 rather than an Err.
    assert_eq!(
        mrb.str_new(b"hello")
            .to_inum(&mrb, 10)
            .expect("non-numeric bytes read 0"),
        0
    );

    // A non-decimal base parses in that radix.
    assert_eq!(
        mrb.str_new(b"ff")
            .to_inum(&mrb, 16)
            .expect("a hex string parses"),
        255
    );
}

#[test]
fn to_inum_surfaces_an_illegal_radix_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A radix outside the 2-through-36 / 0-prefix domain is the one input
    // the lenient parse cannot interpret, so it raises ArgumentError even
    // with strict checking off — protect catches it into Err.
    assert!(matches!(
        mrb.str_new(b"10").to_inum(&mrb, 99),
        Err(Error::Exception(_))
    ));
}

#[test]
fn to_f_parses_a_float_string() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A clean float string parses to its value.
    assert_eq!(
        mrb.str_new(b"2.5")
            .to_f(&mrb)
            .expect("a float string parses"),
        2.5
    );

    // Scientific notation parses too.
    assert_eq!(
        mrb.str_new(b"123.45e1")
            .to_f(&mrb)
            .expect("a scientific-notation string parses"),
        1234.5
    );
}

#[test]
fn to_f_surfaces_invalid_input_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // Trailing junk is rejected by the strict parse — unlike Ruby's
    // lenient String#to_f, which would ignore the trailing characters.
    assert!(matches!(
        mrb.str_new(b"45.67 degrees").to_f(&mrb),
        Err(Error::Exception(_))
    ));

    // Bytes with no valid float at all raise too.
    assert!(matches!(
        mrb.str_new(b"thx1138").to_f(&mrb),
        Err(Error::Exception(_))
    ));
}
