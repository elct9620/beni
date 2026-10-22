//! The Integer handles split by tag: `Fixnum` for the fixed-width one,
//! `RBignum` for the arbitrary-width one a bigint archive allocates.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Fixnum, Integer, Mrb, TryConvert, Value};

fn eval(mrb: &Mrb, source: &str) -> Value {
    mrb.eval::<beni::Value>(source)
        .unwrap_or_else(|err| panic!("{source} raised: {}", err.message(mrb)))
}

/// The class and message of the `Err` converting `source`'s value into `T`.
fn rejection<T: TryConvert>(mrb: &Mrb, source: &str) -> (String, String) {
    match T::try_convert(eval(mrb, source), mrb) {
        Ok(_) => panic!("{source} must not convert"),
        Err(err) => {
            let beni::Error::Exception(exc) = &err else {
                panic!("{source} must surface an exception")
            };
            (exc.classname(mrb), err.message(mrb))
        }
    }
}

#[test]
fn a_fixed_width_integer_converts_into_a_fixnum() {
    let mrb = open_mrb();
    let small = eval(&mrb, "42");

    assert!(Fixnum::from_value(small).is_some());
    assert!(Integer::from_value(small).is_some());
    assert!(Fixnum::from_value(eval(&mrb, "4.2")).is_none());
    // A Float argument truncates, as the Integer handle takes it.
    assert!(Fixnum::try_convert(eval(&mrb, "3.9"), &mrb).is_ok());
    assert_eq!(
        rejection::<Fixnum>(&mrb, "nil"),
        (
            "TypeError".to_owned(),
            "nil cannot be converted to Integer".to_owned()
        )
    );
}

#[cfg(mrb_bigint)]
mod bigint {
    use super::{eval, integer, read, rejection};
    use crate::support::open_mrb;
    use beni::prelude::*;
    use beni::{Fixnum, Integer, IntoValue, RBignum, TryConvert};

    fn range_error(message: &str) -> (String, String) {
        ("RangeError".to_owned(), message.to_owned())
    }

    #[test]
    fn an_arbitrary_width_integer_converts_into_an_rbignum_and_no_fixnum() {
        let mrb = open_mrb();
        let big = eval(&mrb, "2 ** 100");

        assert!(RBignum::from_value(big).is_some());
        assert!(Integer::from_value(big).is_some());
        assert!(Fixnum::from_value(big).is_none());
        assert!(RBignum::from_value(eval(&mrb, "42")).is_none());
    }

    #[test]
    fn each_tag_handle_rejects_an_argument_of_the_other_tag() {
        let mrb = open_mrb();

        assert_eq!(
            rejection::<Fixnum>(&mrb, "2 ** 100"),
            range_error("integer too big for fixnum")
        );
        assert_eq!(
            rejection::<RBignum>(&mrb, "42"),
            range_error("integer too small for bignum")
        );
    }

    #[test]
    fn an_arbitrary_width_integer_reads_out_as_every_rust_integer_its_range_holds() {
        let mrb = open_mrb();

        assert_eq!(read::<u64>(&mrb, "2 ** 64 - 1"), u64::MAX);
        assert_eq!(read::<u128>(&mrb, "2 ** 100"), 1u128 << 100);
        assert_eq!(read::<i128>(&mrb, "-(2 ** 100)"), -(1i128 << 100));
        // A 32-bit configured integer width already outgrows the
        // fixed-width tag here; a 64-bit one keeps it.
        assert_eq!(read::<i64>(&mrb, "2 ** 40"), 1i64 << 40);
        let big = RBignum::from_value(eval(&mrb, "2 ** 64 - 1")).expect("an RBignum");
        assert_eq!(big.to_u64(&mrb).expect("fits"), u64::MAX);
        assert_eq!(big.to_u128(&mrb).expect("fits"), u128::from(u64::MAX));
        assert_eq!(big.to_i128(&mrb).expect("fits"), i128::from(u64::MAX));
    }

    #[test]
    fn the_most_negative_i64_reads_out_whichever_tag_carries_it() {
        let mrb = open_mrb();

        assert_eq!(read::<i64>(&mrb, "-(2 ** 63)"), i64::MIN);
        assert_eq!(
            integer(&mrb, "-(2 ** 63)").to_i64(&mrb).expect("fits"),
            i64::MIN
        );
        assert_eq!(
            rejection::<i64>(&mrb, "-(2 ** 63) - 1"),
            range_error("integer out of range")
        );
    }

    #[test]
    fn an_arbitrary_width_integer_outside_the_target_surfaces_mrubys_wording() {
        let mrb = open_mrb();

        assert_eq!(
            rejection::<u64>(&mrb, "2 ** 64"),
            range_error("integer out of range")
        );
        assert_eq!(
            rejection::<u64>(&mrb, "-(2 ** 64)"),
            range_error("integer out of range")
        );
        assert_eq!(
            rejection::<i64>(&mrb, "2 ** 63"),
            range_error("integer out of range")
        );
        assert_eq!(
            rejection::<u128>(&mrb, "2 ** 128"),
            range_error("integer out of range")
        );
        assert_eq!(
            rejection::<u128>(&mrb, "-(2 ** 100)"),
            range_error("integer out of range")
        );
        assert_eq!(
            rejection::<i8>(&mrb, "2 ** 100"),
            range_error("integer out of range")
        );
        let big = RBignum::from_value(eval(&mrb, "2 ** 100")).expect("an RBignum");
        assert!(big.to_i32(&mrb).is_err());
        assert!(big.to_usize(&mrb).is_err());
    }

    /// Whether `value` is the Integer `source` evaluates to, by Ruby's `==`.
    fn is(mrb: &beni::Mrb, value: beni::Value, source: &str) -> bool {
        value
            .equal(mrb, eval(mrb, source))
            .expect("Integer#== raises nothing")
    }

    #[test]
    fn a_rust_integer_past_the_width_builds_an_arbitrary_width_integer() {
        let mrb = open_mrb();

        let max = mrb.integer_from_u64(u64::MAX);
        assert!(RBignum::from_value(max.as_value()).is_some());
        assert!(is(&mrb, max.as_value(), "2 ** 64 - 1"));
        assert!(is(
            &mrb,
            mrb.integer_from_i128(-(1i128 << 100)).as_value(),
            "-(2 ** 100)"
        ));
        assert!(is(
            &mrb,
            mrb.integer_from_u128(u128::MAX).as_value(),
            "2 ** 128 - 1"
        ));
        assert!(is(
            &mrb,
            mrb.integer_from_i64(-(1i64 << 40)).as_value(),
            "-(2 ** 40)"
        ));
        // A value the width holds stays fixed-width.
        assert!(Fixnum::from_value(mrb.integer_from_u128(42).as_value()).is_some());
    }

    #[test]
    fn a_rust_integer_round_trips_through_its_integer() {
        let mrb = open_mrb();

        for n in [u64::MAX, 1 << 63, 0, 42] {
            assert_eq!(u64::try_convert(n.into_value(&mrb), &mrb).expect("fits"), n);
        }
        for n in [i64::MIN, i64::MAX, -1, 0] {
            assert_eq!(i64::try_convert(n.into_value(&mrb), &mrb).expect("fits"), n);
        }
        for n in [i128::MIN, i128::MAX, -(1 << 100), 7] {
            assert_eq!(
                i128::try_convert(n.into_value(&mrb), &mrb).expect("fits"),
                n
            );
        }
        assert_eq!(
            u128::try_convert(u128::MAX.into_value(&mrb), &mrb).expect("fits"),
            u128::MAX
        );
        assert_eq!(
            usize::try_convert(usize::MAX.into_value(&mrb), &mrb).expect("fits"),
            usize::MAX
        );
    }

    #[test]
    fn a_tag_construction_answers_the_other_tag_where_the_value_needs_it() {
        let mrb = open_mrb();

        let Ok(fix) = mrb.fixnum_from_u64(42) else {
            panic!("42 is fixed-width")
        };
        assert_eq!(fix.to_i64(), 42);
        let Err(big) = mrb.fixnum_from_u64(u64::MAX) else {
            panic!("u64::MAX is arbitrary-width")
        };
        assert_eq!(big.to_u64(&mrb).expect("fits"), u64::MAX);
        let Err(fix) = mrb.bignum_from_i64(-42) else {
            panic!("-42 is fixed-width")
        };
        assert_eq!(fix.to_i64(), -42);
        assert!(mrb.bignum_from_u64(u64::MAX).is_ok());
    }

    #[test]
    fn an_rbignum_answers_its_sign() {
        let mrb = open_mrb();
        let positive = RBignum::from_value(eval(&mrb, "2 ** 100")).expect("an RBignum");
        let negative = RBignum::from_value(eval(&mrb, "-(2 ** 100)")).expect("an RBignum");

        assert!(positive.is_positive(&mrb));
        assert!(!positive.is_negative(&mrb));
        assert!(negative.is_negative(&mrb));
        assert!(!negative.is_positive(&mrb));
    }
}

fn read<T: TryConvert>(mrb: &Mrb, source: &str) -> T {
    T::try_convert(eval(mrb, source), mrb)
        .unwrap_or_else(|err| panic!("{source} must convert: {}", err.message(mrb)))
}

fn integer(mrb: &Mrb, source: &str) -> Integer {
    Integer::from_value(eval(mrb, source)).expect("an Integer")
}

#[test]
fn a_fixed_width_integer_reads_out_as_every_rust_integer_its_range_holds() {
    let mrb = open_mrb();
    let fixnum = Fixnum::from_value(eval(&mrb, "-7")).expect("a Fixnum");

    assert_eq!(fixnum.to_i64(), -7);
    assert_eq!(read::<i32>(&mrb, "-7"), -7);
    assert_eq!(fixnum.to_i128(), -7);
    assert_eq!(fixnum.to_i8(&mrb).expect("fits"), -7);
    assert_eq!(fixnum.to_isize(&mrb).expect("fits"), -7);
    assert_eq!(integer(&mrb, "300").to_u16(&mrb).expect("fits"), 300);
    assert_eq!(integer(&mrb, "300").to_u128(&mrb).expect("fits"), 300);
}

#[test]
fn a_fixed_width_integer_outside_the_target_surfaces_its_value_out_of_range() {
    let mrb = open_mrb();
    let out_of_range = |result: Result<u8, beni::Error>| match result {
        Ok(n) => panic!("{n} must not fit"),
        Err(err) => err.message(&mrb),
    };

    assert_eq!(
        out_of_range(integer(&mrb, "300").to_u8(&mrb)),
        "300 out of range"
    );
    assert_eq!(
        out_of_range(integer(&mrb, "-1").to_u8(&mrb)),
        "-1 out of range"
    );
    assert!(integer(&mrb, "-1").to_u64(&mrb).is_err());
    assert_eq!(
        rejection::<i8>(&mrb, "300"),
        ("RangeError".to_owned(), "300 out of range".to_owned())
    );
}
