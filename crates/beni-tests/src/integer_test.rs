//! The Integer handles split by tag: `Fixnum` for the fixed-width one,
//! `RBignum` for the arbitrary-width one a bigint archive allocates.

use crate::support::open_mrb;
use beni::prelude::*;
use beni::{Fixnum, Integer, Mrb, TryConvert, Value};

fn eval(mrb: &Mrb, source: &str) -> Value {
    mrb.load_string(source.as_bytes())
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
    use super::{eval, rejection};
    use crate::support::open_mrb;
    use beni::prelude::*;
    use beni::{Fixnum, Integer, RBignum};

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
