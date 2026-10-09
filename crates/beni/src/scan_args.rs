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

use crate::state::args::{capture_all_kwargs, slice_from_argv, ArgsCopy};
use crate::try_convert::argument_error;
use crate::{
    Error, FromValue, IntoId, Mrb, Proc, RArray, RHash, ReprValue, Symbol, TryConvert,
    TryConvertOwned, Value,
};
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

/// The most values one part holds: the largest tuple a required or
/// optional part implements below.
const MAX_PARTS: usize = 9;

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
        fn from_bucket(bucket: Option<RHash>) -> Self;
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
            impl<$($t: TryConvert),+> ScanArgsRequired for ($($t,)+) {
                const LEN: usize = $len;

                fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
                    Ok(($($t::try_convert(vals[$i], mrb)?,)+))
                }
            }

            impl<$($t: TryConvert),+> ScanArgsOpt for ($(Option<$t>,)+) {
                const LEN: usize = $len;

                fn from_options(mrb: &Mrb, vals: &[Option<Value>]) -> Result<Self, Error> {
                    Ok(($(vals[$i].map(|v| $t::try_convert(v, mrb)).transpose()?,)+))
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

    impl ScanArgsSplat for RArray {
        const REQ: bool = true;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
            Ok(mrb.ary_new_from_values(vals))
        }
    }

    impl<T: TryConvertOwned> ScanArgsSplat for Vec<T> {
        const REQ: bool = true;

        fn from_slice(mrb: &Mrb, vals: &[Value]) -> Result<Self, Error> {
            vals.iter().map(|v| T::try_convert(*v, mrb)).collect()
        }
    }

    impl ScanArgsKw for () {
        const REQ: bool = false;

        fn from_bucket(_: Option<RHash>) -> Self {}
    }

    impl ScanArgsKw for RHash {
        const REQ: bool = true;

        fn from_bucket(bucket: Option<RHash>) -> Self {
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
        fn from_block(_: &Mrb, block: Value) -> Result<Self, Error> {
            Ok(Proc::from_value(block))
        }
    }
}

/// Required positionals of `scan_args`, or required keywords of
/// `get_kwargs`: `()`, or a tuple of up to nine `TryConvert` types.
pub trait ScanArgsRequired: private::ScanArgsRequired {}
impl<T: private::ScanArgsRequired> ScanArgsRequired for T {}

/// Optional positionals of `scan_args`, or optional keywords of
/// `get_kwargs`: `()`, or a tuple of up to nine `Option`s of `TryConvert`
/// types.
pub trait ScanArgsOpt: private::ScanArgsOpt {}
impl<T: private::ScanArgsOpt> ScanArgsOpt for T {}

/// The splat of `scan_args`: `()`, an `RArray` handle, or a `Vec` of a
/// `TryConvert` type.
pub trait ScanArgsSplat: private::ScanArgsSplat {}
impl<T: private::ScanArgsSplat> ScanArgsSplat for T {}

/// The keyword bucket of `scan_args`, or the unnamed keywords of
/// `get_kwargs`: `()` or an `RHash`.
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
    let mut slots = [None; MAX_PARTS];
    for (slot, value) in slots.iter_mut().zip(optional) {
        *slot = Some(*value);
    }
    let (splat, trailing) = rest.split_at(rest.len() - Trail::LEN);
    Ok(Args {
        required: Req::from_slice(mrb, required)?,
        optional: Opt::from_options(mrb, &slots[..Opt::LEN])?,
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
/// let rest: RHash = kw.splat;
/// ```
pub fn get_kwargs<K, Req, Opt, Splat>(
    mrb: &Mrb,
    kw: RHash,
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

    let mut required_values = [crate::value::qnil().as_value(); MAX_PARTS];
    for (slot, &name) in required_values.iter_mut().zip(required) {
        match take(name)? {
            (_, Some(value)) => *slot = value,
            (key, None) => {
                let name = key.to_string(mrb);
                return Err(argument_error(mrb, &format!("missing keyword: {name}")));
            }
        }
    }
    let mut optional_values = [None; MAX_PARTS];
    for (slot, &name) in optional_values.iter_mut().zip(optional) {
        *slot = take(name)?.1;
    }
    if !Splat::REQ && !rest.is_empty(mrb) {
        let key = rest.keys(mrb).entry(mrb, 0).to_string(mrb);
        return Err(argument_error(mrb, &format!("unknown keyword: {key}")));
    }

    Ok(KwArgs {
        required: Req::from_slice(mrb, &required_values[..Req::LEN])?,
        optional: Opt::from_options(mrb, &optional_values[..Opt::LEN])?,
        splat: Splat::from_bucket(Splat::REQ.then_some(rest)),
    })
}

/// The call frame as one read sees it: the positionals copied out of the
/// frame that keeps their values alive, the keyword bucket, and the block
/// slot.
pub(crate) struct Frame {
    pub(crate) positionals: ArgsCopy,
    pub(crate) keywords: Option<RHash>,
    pub(crate) block: Value,
}

/// Read the frame once, keeping the keywords in their own bucket when
/// `keywords` is set, which leaves the frame as it was, and otherwise
/// letting mruby fold a non-empty keyword hash into the positionals.
pub(crate) fn read_call(mrb: &Mrb, keywords: bool) -> Frame {
    let call = read_raw(mrb, keywords);
    Frame {
        positionals: ArgsCopy::new(call.positionals, None),
        keywords: call.keywords,
        block: call.block,
    }
}

/// As `Frame`, the positionals borrowed from the VM stack: valid only
/// until the body next re-enters the VM, so a caller copies them out
/// before running anything.
pub(crate) struct RawFrame<'a> {
    pub(crate) positionals: &'a [Value],
    pub(crate) keywords: Option<RHash>,
    pub(crate) block: Value,
}

/// `read_call` without the copy.
pub(crate) fn read_raw<'a>(mrb: &'a Mrb, keywords: bool) -> RawFrame<'a> {
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
    RawFrame {
        positionals: slice_from_argv(argv, argc),
        // SAFETY: the capture-all read fills `bucket` with a Hash, an
        // empty one when the call passed no keywords.
        keywords: keywords
            .then(|| unsafe { RHash::from_value_unchecked(Value::from_raw_unchecked(bucket)) }),
        block: Value::from_raw_unchecked(block),
    }
}

/// The `ArgumentError` mruby raises for `given` positionals against `min`
/// and at most `max` of them.
pub(crate) fn argnum_error(mrb: &Mrb, given: usize, min: usize, max: Option<usize>) -> Error {
    Error::argnum(mrb, given, min as i32, max.map_or(-1, |max| max as i32))
}
