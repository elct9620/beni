//! Handles on the classes mruby's bundled gems define, mirroring
//! magnus's `Time`, `RFile`, and `Enumerator`. Each converts through
//! `TryConvert` alone: telling one needs the class, which only an
//! interpreter whose archive carries the gem binds.

use crate::{Error, Mrb, RClass, ReprValue, TryConvert, Value};
use beni_sys as sys;

/// The class bound to the top-level constant `name`, read straight from
/// `Object`'s constant table so no `const_missing` runs; `None` when no
/// gem defined it or the constant names something else.
fn top_level_class(mrb: &Mrb, name: &[u8]) -> Option<RClass> {
    let id = mrb.check_id(name)?;
    // SAFETY: `mrb` is alive and `Object` is its live class; the read
    // dispatches nothing and answers `nil` for an unbound name.
    let bound = Value::from_raw_unchecked(unsafe {
        sys::mrb_obj_iv_get(
            mrb.as_ptr(),
            mrb.class_object().as_internal().cast(),
            id.to_raw(),
        )
    });
    <RClass as crate::FromValue>::from_value(bound)
}

macro_rules! bundled_handle {
    ($($(#[$doc:meta])* $name:ident => $class:literal),* $(,)?) => {$(
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Copy, Clone)]
        pub struct $name(Value);

        impl $name {
            /// Wrap a `Value` the caller has established is an instance of
            /// the handle's class.
            ///
            /// # Safety
            ///
            /// `v` must be an instance of the handle's class.
            #[inline]
            pub unsafe fn from_value_unchecked(v: Value) -> Self {
                Self(v)
            }
        }

        crate::value::value_backed_repr!($name);

        impl TryConvert for $name {
            fn try_convert(val: Value, mrb: &Mrb) -> Result<Self, Error> {
                match top_level_class(mrb, $class.as_bytes()) {
                    // SAFETY: the kind check just held.
                    Some(class) if val.is_kind_of(mrb, class) => {
                        Ok(unsafe { Self::from_value_unchecked(val) })
                    }
                    _ => Err(crate::try_convert::not_convertible(val, mrb, $class)),
                }
            }
        }
    )*};
}

bundled_handle!(
    /// An instance of `Time`, from mruby's time gem. Mirrors magnus's
    /// `Time`.
    Time => "Time",
    /// An instance of `IO`, a `File` included, from mruby's io gem.
    /// Mirrors magnus's `RFile`, which CRuby tags every IO with.
    RFile => "IO",
    /// An instance of `Enumerator`, from mruby's enumerator gem. Mirrors
    /// magnus's `Enumerator`.
    Enumerator => "Enumerator",
);
