use beni::{Ccontext, Error, FromValue, IntoValue, Mrb, Range, RangeBegLen};

#[test]
fn range_new_constructs_and_reads_back_its_bounds() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // An inclusive integer range round-trips its begin, end, and
    // exclude-end flag.
    let r = mrb
        .range_new(0.into_value(&mrb), 10.into_value(&mrb), false)
        .expect("a comparable integer range constructs");
    assert_eq!(i32::from_value(r.begin(&mrb)), Some(0));
    assert_eq!(i32::from_value(r.end(&mrb)), Some(10));
    assert!(!r.is_exclusive(&mrb));
}

#[test]
fn range_new_carries_the_exclusive_flag() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    let r = mrb
        .range_new(1.into_value(&mrb), 5.into_value(&mrb), true)
        .expect("a comparable integer range constructs");
    assert!(r.is_exclusive(&mrb));
}

#[test]
fn range_new_surfaces_incomparable_bounds_as_err() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A String and an Integer share no ordering, so mruby raises
    // ArgumentError ("bad value for range") — protect catches it into
    // Err rather than long-jumping across FFI.
    let result = mrb.range_new(mrb.str_new(b"a").as_value(), 42.into_value(&mrb), false);
    assert!(matches!(result, Err(Error::Exception(_))));
}

#[test]
fn from_value_downcasts_by_the_range_tag() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context must succeed");

    // A Range-tagged value downcasts to the typed handle; a non-range
    // tag rejects instead of wrapping a value the range reads would
    // misread.
    let range_val = cxt
        .load_nstring(b"(1..5)")
        .expect("the test source must compile and run");
    assert!(
        mrb.pending_exc().is_nil(),
        "building the range literal must not raise"
    );
    assert!(Range::from_value(range_val).is_some());
    assert!(Range::from_value(42.into_value(&mrb)).is_none());
}

#[test]
fn reads_track_an_exclusive_literal() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context must succeed");

    // A `(1...5)` literal is exclusive; its bounds read back unchanged.
    let r = Range::from_value(
        cxt.load_nstring(b"(1...5)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal is Range-tagged");
    assert_eq!(i32::from_value(r.begin(&mrb)), Some(1));
    assert_eq!(i32::from_value(r.end(&mrb)), Some(5));
    assert!(r.is_exclusive(&mrb));
}

#[test]
fn beg_len_maps_an_in_range_slice() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

    // `2..7` against a length-10 collection selects 6 elements from
    // offset 2 (inclusive end, so 7 - 2 + 1). A negative end counts
    // back from the length.
    let r = Range::from_value(
        cxt.load_nstring(b"(2..7)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal");
    assert_eq!(
        r.beg_len(&mrb, 10, false)
            .expect("an integer range never raises"),
        RangeBegLen::Ok { beg: 2, len: 6 }
    );

    let r = Range::from_value(
        cxt.load_nstring(b"(-3..-1)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal");
    assert_eq!(
        r.beg_len(&mrb, 10, false)
            .expect("an integer range never raises"),
        RangeBegLen::Ok { beg: 7, len: 3 }
    );
}

#[test]
fn beg_len_reports_a_begin_before_the_start_as_out() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

    // `-20` counts back past the start of a length-10 collection, so
    // the begin offset is out of range.
    let r = Range::from_value(
        cxt.load_nstring(b"(-20..-1)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal");
    assert_eq!(
        r.beg_len(&mrb, 10, false)
            .expect("an integer range never raises"),
        RangeBegLen::Out
    );
}

#[test]
fn beg_len_truncates_an_over_long_end() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

    // `2..100` overruns a length-10 collection. With truncation the end
    // clamps to the length, selecting offsets 2 through 9; without it
    // the raw bounds yield a longer span.
    let r = Range::from_value(
        cxt.load_nstring(b"(2..100)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal");
    assert_eq!(
        r.beg_len(&mrb, 10, true)
            .expect("an integer range never raises"),
        RangeBegLen::Ok { beg: 2, len: 8 }
    );
    assert_eq!(
        r.beg_len(&mrb, 10, false)
            .expect("an integer range never raises"),
        RangeBegLen::Ok { beg: 2, len: 99 }
    );
}

#[test]
fn beg_len_saturates_a_length_past_the_mrb_int_width() {
    // A length past `mrb_int` is only representable when the host `i64`
    // is wider than `mrb_int`, i.e. a 32-bit `mrb_int`. Under a 64-bit
    // `mrb_int` no `i64` exceeds its range, so the saturation premise is
    // vacuous and the case is skipped rather than asserted on a width it
    // cannot reach.
    if core::mem::size_of::<beni::sys::mrb_int>() != 4 {
        return;
    }

    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");
    let cxt = Ccontext::new(&mrb, c"range_test.rb").expect("allocating the context");

    // A length wider than `mrb_int` saturates up to the widest
    // representable extent, so truncating `2..7` still selects offsets
    // 2 through 7. A wrapping cast would land on a negative length, and
    // truncation against it would report the begin as out of range.
    let huge = i64::from(beni::sys::mrb_int::MAX) + 1;
    let r = Range::from_value(
        cxt.load_nstring(b"(2..7)")
            .expect("the test source must compile and run"),
    )
    .expect("a Range literal");
    assert_eq!(
        r.beg_len(&mrb, huge, true)
            .expect("an integer range never raises"),
        RangeBegLen::Ok { beg: 2, len: 6 }
    );
}

#[test]
fn beg_len_rejects_a_non_range_as_mismatch() {
    let mrb = Mrb::open().expect("Mrb::open failed with libmruby.a linked");

    // A non-Range receiver wrapped through the unchecked cast reports a
    // type mismatch rather than reading a malformed field.
    let not_a_range = unsafe { Range::from_value_unchecked(42.into_value(&mrb)) };
    assert_eq!(
        not_a_range
            .beg_len(&mrb, 10, false)
            .expect("a mismatch is a return, not a raise"),
        RangeBegLen::TypeMismatch
    );
}
