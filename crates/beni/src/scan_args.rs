//! Typed reads of a method's arguments — beni's mirror of
//! `magnus::scan_args`.
//!
//! A method registered for any arity reads its call frame through
//! `scan_args`, whose type parameters declare the argument shape:
//!
//! ```text
//! def example(a, b, c=nil, d=nil, *rest, e, f, g:, h: nil, **kwargs, &block)
//!             \__/  \__________/  \___/  \__/  \___________________/  \____/
//!           required  optional    splat trailing      keywords          block
//! ```
//!
//! Each part is `()` when the shape has none of it. Where magnus scans an
//! argument slice, `scan_args` reads the frame, because mruby parses
//! arguments only from the frame; the `Args` it answers has magnus's shape.
//! `get_kwargs` takes a keyword bucket apart by name, as magnus's does.

use crate::method::{arg_type_error, core_exception};
use crate::state::args::{capture_all_kwargs, slice_from_argv};
use crate::{Array, Error, FromValue, Hash, IntoId, Mrb, Proc, ReprValue, Symbol, Value};
use beni_sys as sys;

/// The parts `scan_args` hands back, each typed by the parameter that
/// declared it.
pub struct Args<Req, Opt, Splat, Trail, Kw, Block> {
    /// Required positionals.
    pub required: Req,
    /// Optional positionals, `None` for each the call omitted.
    pub optional: Opt,
    /// The positionals between the optional and the trailing ones.
    pub splat: Splat,
    /// Required positionals after the splat.
    pub trailing: Trail,
    /// The keyword bucket.
    pub keywords: Kw,
    /// The block.
    pub block: Block,
}

/// The parts `get_kwargs` hands back, each typed by the parameter that
/// declared it.
pub struct KwArgs<Req, Opt, Splat> {
    /// Values of the required keywords.
    pub required: Req,
    /// Values of the optional keywords, `None` for each the hash lacks.
    pub optional: Opt,
    /// The keywords neither list names.
    pub splat: Splat,
}

mod private {
    use super::*;

    pub trait ScanArgsRequired: Sized {
        const LEN: usize;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error>;
    }

    pub trait ScanArgsOpt: Sized {
        const LEN: usize;

        /// `vals` holds one slot per optional, `None` where it was omitted.
        fn from_options(mrb: &Mrb, vals: &[Option<Value>]) -> Result<Self, Error>;
    }

    pub trait ScanArgsSplat: Sized {
        const REQ: bool;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error>;
    }

    pub trait ScanArgsKw: Sized {
        const REQ: bool;

        /// `bucket` is the keyword bucket exactly when `REQ` is set.
        fn from_bucket(bucket: Option<Hash>) -> Self;
    }

    pub trait ScanArgsBlock: Sized {
        fn from_block(mrb: &Mrb, block: Value) -> Result<Self, Error>;
    }

    impl ScanArgsRequired for () {
        const LEN: usize = 0;

        fn from_slice(_: &Mrb, _: &[Value]) -> Result<Self, Error> {
            Ok(())
        }
    }

    impl ScanArgsOpt for () {
        const LEN: usize = 0;

        fn from_options(_: &Mrb, _: &[Option<Value>]) -> Result<Self, Error> {
            Ok(())
        }
    }

    macro_rules! impl_positional_parts {
        ($len:literal; $($t:ident $i:tt),+) => {
            impl<$($t: FromValue),+> ScanArgsRequired for ($($t,)+) {
                const LEN: usize = $len;

                fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
                    Ok(($(convert::<$t>(mrb, vals[$i])?,)+))
                }
            }

            impl<$($t: FromValue),+> ScanArgsOpt for ($(Option<$t>,)+) {
                const LEN: usize = $len;

                fn from_options(mrb: &Mrb, vals: &[Option<Value>]) -> Result<Self, Error> {
                    Ok(($(vals[$i].map(|v| convert::<$t>(mrb, v)).transpose()?,)+))
                }
            }
        };
    }

    impl_positional_parts!(1; T0 0);
    impl_positional_parts!(2; T0 0, T1 1);
    impl_positional_parts!(3; T0 0, T1 1, T2 2);
    impl_positional_parts!(4; T0 0, T1 1, T2 2, T3 3);
    impl_positional_parts!(5; T0 0, T1 1, T2 2, T3 3, T4 4);
    impl_positional_parts!(6; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
    impl_positional_parts!(7; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
    impl_positional_parts!(8; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);
    impl_positional_parts!(9; T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7, T8 8);

    impl ScanArgsSplat for () {
        const REQ: bool = false;

        fn from_slice(_: &Mrb, _: &[Value]) -> Result<Self, Error> {
            Ok(())
        }
    }

    impl ScanArgsSplat for Array {
        const REQ: bool = true;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
            Ok(mrb.ary_new_from_values(vals))
        }
    }

    impl<T: FromValue> ScanArgsSplat for Vec<T> {
        const REQ: bool = true;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
            vals.iter().map(|v| convert::<T>(mrb, *v)).collect()
        }
    }

    impl ScanArgsKw for () {
        const REQ: bool = false;

        fn from_bucket(_: Option<Hash>) -> Self {}
    }

    impl ScanArgsKw for Hash {
        const REQ: bool = true;

        fn from_bucket(bucket: Option<Hash>) -> Self {
            bucket.expect("a keyword part reads the keyword bucket")
        }
    }

    impl ScanArgsBlock for () {
        fn from_block(_: &Mrb, _: Value) -> Result<Self, Error> {
            Ok(())
        }
    }

    impl ScanArgsBlock for Proc {
        fn from_block(mrb: &Mrb, block: Value) -> Result<Self, Error> {
            Proc::from_value(block).ok_or_else(|| argument_error(mrb, "no block given"))
        }
    }

    impl ScanArgsBlock for Option<Proc> {
        fn from_block(mrb: &Mrb, block: Value) -> Result<Self, Error> {
            convert(mrb, block)
        }
    }
}

/// Required positionals of `scan_args`, or required keywords of
/// `get_kwargs`: `()`, or a tuple of up to nine `FromValue` types.
pub trait ScanArgsRequired: private::ScanArgsRequired {}
impl<T: private::ScanArgsRequired> ScanArgsRequired for T {}

/// Optional positionals of `scan_args`, or optional keywords of
/// `get_kwargs`: `()`, or a tuple of up to nine `Option`s of `FromValue`
/// types.
pub trait ScanArgsOpt: private::ScanArgsOpt {}
impl<T: private::ScanArgsOpt> ScanArgsOpt for T {}

/// The splat of `scan_args`: `()`, an `Array` handle, or a `Vec` of a
/// `FromValue` type.
pub trait ScanArgsSplat: private::ScanArgsSplat {}
impl<T: private::ScanArgsSplat> ScanArgsSplat for T {}

/// The keyword bucket of `scan_args`, or the unnamed keywords of
/// `get_kwargs`: `()` or a `Hash`.
pub trait ScanArgsKw: private::ScanArgsKw {}
impl<T: private::ScanArgsKw> ScanArgsKw for T {}

/// The block of `scan_args`: `()` to ignore it, `Proc` to require it, or
/// `Option<Proc>` to accept it.
pub trait ScanArgsBlock: private::ScanArgsBlock {}
impl<T: private::ScanArgsBlock> ScanArgsBlock for T {}

/// Read the current call's arguments into the shape the type parameters
/// declare, or answer the `Err` carrying the `ArgumentError` or
/// `TypeError` for a call that does not fit it. Without a keyword part a
/// non-empty keyword hash reads as the last positional, and every later
/// read in the call sees it there. Mirrors magnus's `scan_args`.
///
/// ```ignore
/// // def new(hostname = nil, port)
/// let args = beni::scan_args::scan_args::<(), (Option<String>,), (), (u16,), (), ()>(mrb)?;
/// let (hostname,) = args.optional;
/// let (port,) = args.trailing;
/// ```
pub fn scan_args<Req, Opt, Splat, Trail, Kw, Block>(
    mrb: &Mrb,
) -> Result<Args<Req, Opt, Splat, Trail, Kw, Block>, Error>
where
    Req: ScanArgsRequired,
    Opt: ScanArgsOpt,
    Splat: ScanArgsSplat,
    Trail: ScanArgsRequired,
    Kw: ScanArgsKw,
    Block: ScanArgsBlock,
{
    let call = read_call(mrb, Kw::REQ);
    let positionals = call.positionals.as_slice();
    let fixed = Req::LEN + Trail::LEN;
    let max = (!Splat::REQ).then_some(fixed + Opt::LEN);
    if positionals.len() < fixed || max.is_some_and(|max| positionals.len() > max) {
        return Err(argnum_error(mrb, positionals.len(), fixed, max));
    }

    let supplied = (positionals.len() - fixed).min(Opt::LEN);
    let (required, rest) = positionals.split_at(Req::LEN);
    let (optional, rest) = rest.split_at(supplied);
    let optional: Vec<Option<Value>> = (0..Opt::LEN).map(|i| optional.get(i).copied()).collect();
    let (splat, trailing) = rest.split_at(rest.len() - Trail::LEN);
    Ok(Args {
        required: Req::from_slice(mrb, required)?,
        optional: Opt::from_options(mrb, &optional)?,
        splat: Splat::from_slice(mrb, splat)?,
        trailing: Trail::from_slice(mrb, trailing)?,
        keywords: Kw::from_bucket(call.keywords),
        block: Block::from_block(mrb, call.block)?,
    })
}

/// Take the keywords `required` and `optional` name out of `kw` into the
/// shape the type parameters declare, leaving `kw` unchanged. A required
/// keyword `kw` lacks, or a keyword neither list names when `Splat` is `()`,
/// answers the `Err` carrying an `ArgumentError`, and a value of the wrong
/// type one carrying a `TypeError`. Mirrors magnus's `get_kwargs`.
///
/// # Panics
///
/// When `required` or `optional` differs in length from the count `Req` or
/// `Opt` declares.
///
/// ```ignore
/// // def test(a:, b:, c: nil, **rest)
/// let kw = beni::scan_args::get_kwargs(mrb, bucket, &["a", "b"], &["c"])?;
/// let (a, b): (String, usize) = kw.required;
/// let (c,): (Option<bool>,) = kw.optional;
/// let rest: Hash = kw.splat;
/// ```
pub fn get_kwargs<K, Req, Opt, Splat>(
    mrb: &Mrb,
    kw: Hash,
    required: &[K],
    optional: &[K],
) -> Result<KwArgs<Req, Opt, Splat>, Error>
where
    K: IntoId + Copy,
    Req: ScanArgsRequired,
    Opt: ScanArgsOpt,
    Splat: ScanArgsKw,
{
    assert_eq!(required.len(), Req::LEN, "one name per required keyword");
    assert_eq!(optional.len(), Opt::LEN, "one name per optional keyword");
    let rest = kw.dup(mrb);
    let take = |name: K| -> Result<(Value, Option<Value>), Error> {
        let key = Symbol::from(name.into_id(mrb)?).as_value();
        let value = if rest.contains_key(mrb, key)? {
            Some(rest.delete(mrb, key)?)
        } else {
            None
        };
        Ok((key, value))
    };

    let mut required_values = Vec::with_capacity(required.len());
    for &name in required {
        match take(name)? {
            (_, Some(value)) => required_values.push(value),
            (key, None) => {
                let name = key.to_string(mrb);
                return Err(argument_error(mrb, &format!("missing keyword: {name}")));
            }
        }
    }
    let optional_values = optional
        .iter()
        .map(|&name| take(name).map(|(_, value)| value))
        .collect::<Result<Vec<_>, _>>()?;
    if !Splat::REQ && !rest.is_empty(mrb) {
        let key = rest.keys(mrb).entry(0).to_string(mrb);
        return Err(argument_error(mrb, &format!("unknown keyword: {key}")));
    }

    Ok(KwArgs {
        required: Req::from_slice(mrb, &required_values)?,
        optional: Opt::from_options(mrb, &optional_values)?,
        splat: Splat::from_bucket(Splat::REQ.then_some(rest)),
    })
}

/// The call frame as one read sees it: the positionals copied out of the
/// frame that keeps their values alive, the keyword bucket, and the block
/// slot.
pub(crate) struct Frame {
    pub(crate) positionals: Vec<Value>,
    pub(crate) keywords: Option<Hash>,
    pub(crate) block: Value,
}

/// Read the frame once, keeping the keywords in their own bucket when
/// `keywords` is set, which leaves the frame as it was, and otherwise
/// letting mruby fold a non-empty keyword hash into the positionals.
pub(crate) fn read_call(mrb: &Mrb, keywords: bool) -> Frame {
    let mut argv: *const sys::mrb_value = core::ptr::null();
    let mut argc: sys::mrb_int = 0;
    let mut bucket = sys::mrb_value::zeroed();
    let mut kwargs = capture_all_kwargs(&mut bucket);
    let mut block = sys::mrb_value::zeroed();
    // SAFETY: `mrb` is alive by the borrow. Neither format can raise: the
    // uncopied rest accepts every count, the capture-all keyword read
    // rejects nothing, and the plain block read never checks. Each writes
    // the argv pointer + length pair, the capture-all bucket through
    // `kwargs` for `":"`, and the block slot.
    unsafe {
        if keywords {
            sys::mrb_get_args(
                mrb.as_ptr(),
                c"*!:&".as_ptr(),
                &mut argv as *mut *const sys::mrb_value,
                &mut argc as *mut sys::mrb_int,
                &mut kwargs as *mut sys::mrb_kwargs,
                &mut block as *mut sys::mrb_value,
            );
        } else {
            sys::mrb_get_args(
                mrb.as_ptr(),
                c"*!&".as_ptr(),
                &mut argv as *mut *const sys::mrb_value,
                &mut argc as *mut sys::mrb_int,
                &mut block as *mut sys::mrb_value,
            );
        }
    }
    Frame {
        positionals: slice_from_argv(argv, argc).to_vec(),
        // SAFETY: the capture-all read fills `bucket` with a Hash, an
        // empty one when the call passed no keywords.
        keywords: keywords
            .then(|| unsafe { Hash::from_value_unchecked(Value::from_raw_unchecked(bucket)) }),
        block: Value::from_raw_unchecked(block),
    }
}

/// The `ArgumentError` mruby raises for `given` positionals against `min`
/// and at most `max` of them.
fn argnum_error(mrb: &Mrb, given: usize, min: usize, max: Option<usize>) -> Error {
    let max = max.map_or(-1, |max| max as core::ffi::c_int);
    // SAFETY: `mrb` is alive inside the protect frame, which catches the
    // raise `mrb_argnum_error` always ends in.
    let raised = mrb.protect(|mrb| -> Value {
        unsafe {
            sys::mrb_argnum_error(
                mrb.as_ptr(),
                given as sys::mrb_int,
                min as core::ffi::c_int,
                max,
            )
        }
    });
    match raised {
        Err(err) => err,
        Ok(_) => unreachable!("mrb_argnum_error always raises"),
    }
}

fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    Error::Exception(core_exception(mrb, c"ArgumentError", msg))
}

fn convert<T: FromValue>(mrb: &Mrb, value: Value) -> Result<T, Error> {
    T::from_value(value).ok_or_else(|| arg_type_error::<T>(mrb))
}
