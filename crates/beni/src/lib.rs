//! beni — typed Rust wrapper over the `beni-sys` FFI surface.
//!
//! This crate owns every Rust-level abstraction above the mruby C
//! API: the `Mrb` / `Ccontext` RAII types, the `Value` / `RClass` /
//! `Array` / `Hash` newtypes, the `IntoValue` / `FromValue` trait
//! seam, the `Format`-based `mrb_get_args` dispatch, and the
//! `protect` closure wrapper. The sibling `beni-sys` crate keeps
//! only the bindgen-generated `extern "C"` declarations and the
//! layout-safe C shims — the same split magnus + rb-sys apply at
//! the CRuby boundary.
//!
//! ## Layering
//!
//! ```text
//! L2  trait seams      value::convert (IntoValue / FromValue)
//!                      state::args    (Format trait + ZST + GAT dispatch)
//!                      state::protect (closure-based mrb_protect_error)
//!                      method         (method! bridges + MethodN crossing)
//!                      gem            (Gem trait + Mrb::init_gem)
//!
//! L1  RAII / newtypes  state         (Mrb owning *mut mrb_state)
//!                      value         (Value newtype + cstr! / cstr_ptr)
//!                      class         (RClass / RModule handles + traits)
//!                      array / hash  (typed factories on top of Value)
//!                      ccontext      (Ccontext RAII)
//!
//! L0  raw FFI          beni-sys::*  (bindgen output + ABI constants)
//! ```
//!
//! ## Raw-FFI escape hatch
//!
//! `beni::sys` re-exports the entire `beni-sys` crate so call
//! sites that still need the raw bindgen surface
//! (`sys::mrb_value`, `sys::mrb_state`, `sys::mrb_func_t`,
//! `sys::mrb_args_*`, …) keep a short import path. Anything that
//! becomes wrappable in the typed surface above should leave this
//! escape hatch over time.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

// Safe-layer modules. These hold the typed abstractions over the
// bindgen FFI surface: `Mrb` / `Ccontext` RAII, typed `Value` /
// `RClass` / `RModule` / `Array` / `Hash` newtypes, and the `cstr!` / `cstr_ptr`
// C-string helpers.
//
// Every module and re-export is unconditional: in placeholder mode
// the full API surface still compiles (the spec's transitive-
// dependency guarantee), with mruby-calling method bodies diverting
// to `not_linked` — see that helper below.
pub mod array;
pub mod ccontext;
pub mod class;
pub mod convert;
pub mod error;
pub mod gem;
pub mod hash;
pub mod method;
pub mod state;
pub mod value;

pub use state::{Mrb, MrbOpenError};

pub use state::args::{format, Format};

pub use ccontext::Ccontext;

pub use array::Array;
pub use class::{Module, Object, RClass, RModule};
pub use convert::{FromValue, IntoValue};
pub use error::Error;
pub use gem::Gem;
pub use hash::Hash;
pub use method::{MethodDef, MethodReturn};
pub use value::cstr_ptr;
pub use value::Value;

/// Placeholder-mode terminus for operations that need a linked
/// mruby. Methods taking `&Mrb` can never reach it (`Mrb::open`
/// returns `Err`, so no `Mrb` exists to borrow); pure value methods
/// reach it only when called on a degenerate placeholder value.
#[cfg(not(mruby_linked))]
#[inline]
pub(crate) fn not_linked() -> ! {
    panic!(
        "beni placeholder mode: mruby is not linked; this operation needs a discovered libmruby.a"
    )
}

/// Raw FFI escape hatch. Use `beni::sys::mrb_*` when the typed API
/// in this crate's root does not yet cover a needed symbol. Anything
/// promoted out of this namespace into the typed surface should
/// disappear from new call sites over time.
pub use beni_sys as sys;

/// Typed counterpart of `sys::mrb_func_t` using the `Value` newtype
/// for the receiver and return slots. `Value` is
/// `#[repr(transparent)]` over `mrb_value`, so this alias has the
/// same C ABI as `sys::mrb_func_t` — but Rust nominal typing keeps
/// the two distinct, which lets `Module::define_method` accept
/// bridges declared with the ergonomic typed signature without an
/// `as`-cast at every call site. The `transmute` from this typed
/// alias to `sys::mrb_func_t` happens once inside
/// `Module::define_method` / `Object::define_singleton_method`.
///
/// Unconditional (not `mruby_linked`-gated) so the sanity test
/// `typed_mrb_func_t_coerces_from_value_bridge` (in `tests` below)
/// pins the bridge signature → typed alias coercion at compile time
/// even in placeholder builds, against the placeholder
/// `mrb_state` / `mrb_value` types.
pub type mrb_func_t = unsafe extern "C" fn(mrb: *mut sys::mrb_state, self_: Value) -> Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_api_surface_compiles_in_both_modes() {
        // Compile-surface regression net for the placeholder
        // contract: the wrapper's full API surface compiles whether
        // or not mruby is linked. Each binding references one public
        // method path; a method that loses its placeholder branch
        // breaks this test's compilation in the CI lint lane's
        // placeholder build before any consumer sees it.
        let _ = Mrb::open;
        let _ = Mrb::as_ptr;
        let _ = Mrb::borrow_raw;
        let _ = Mrb::pending_exc;
        let _ = Mrb::set_pending_exc;
        let _ = Mrb::clear_exc;
        let _ = Mrb::object_class;
        let _ = Mrb::define_module;
        let _ = Mrb::define_class;
        let _ = Mrb::class_get;
        let _ = Mrb::define_global_const;
        let _ = Mrb::gv_set;
        let _ = Mrb::gv_get;
        let _ = Mrb::str_new;
        let _ = Mrb::str_new_cstr;
        let _ = Mrb::ary_new;
        let _ = Mrb::hash_new;
        let _ = Mrb::intern_cstr;
        let _ = Mrb::intern_str;
        let _ = Mrb::sym_name;
        let _ = Mrb::load_irep_buf;
        let _ = Mrb::load_bytecode;
        let _ = Mrb::protect::<fn(&Mrb) -> Value>;
        let _ = Mrb::get_args::<format::O>;
        let _ = <format::O as Format>::read;
        let _ = <format::Rest as Format>::read;
        let _ = <format::NRest as Format>::read;
        let _ = <format::NRestBlock as Format>::read;
        let _ = <format::Io as Format>::read;
        let _ = Value::nil;
        let _ = Value::true_;
        let _ = Value::false_;
        let _ = Value::from_int;
        let _ = Value::from_float;
        let _ = Value::obj_as_string;
        let _ = Value::as_bytes;
        let _ = Value::classname;
        let _ = Value::to_string;
        let _ = Value::as_class_ptr;
        let _ = Value::call;
        let _ = Value::call_argv;
        let _ = Value::is_nil;
        let _ = Value::is_integer;
        let _ = Value::is_float;
        let _ = Value::unbox_integer;
        let _ = Value::unbox_float;
        let _ = Value::ary_entry;
        let _ = Value::iv_set;
        let _ = Value::iv_get;
        let _ = Value::const_defined;
        let _ = Value::const_get;
        let _ = Value::respond_to;
        let _ = RClass::as_value;
        let _ = RClass::obj_new;
        let _ = RClass::raise;
        let _ = RClass::is_null;
        let _ = RModule::from_raw;
        let _ = RModule::as_raw;
        let _ = <RClass as Module>::define_class;
        let _ = <RClass as Module>::define_module;
        let _ = <RClass as Module>::class_get;
        let _ = <RClass as Module>::define_method;
        let _ = <RClass as Module>::name;
        let _ = <RModule as Module>::define_class;
        let _ = <RModule as Module>::define_method;
        let _ = <RClass as Object>::define_singleton_method;
        let _ = <RModule as Object>::define_singleton_method;
        let _ = Error::message;
        let _ = MethodDef::new;
        struct _SurfaceGem;
        impl Gem for _SurfaceGem {
            fn init(_mrb: &Mrb) -> Result<(), Error> {
                Ok(())
            }
        }
        let _ = Mrb::init_gem::<_SurfaceGem>;
        let _ = <_SurfaceGem as Gem>::init;
        fn _surface_typed(_mrb: &Mrb, _self: Value, _a: i32) -> i32 {
            0
        }
        fn _surface_any(_mrb: &Mrb, _self: Value) -> Value {
            Value::zeroed()
        }
        let _ = crate::method!(_surface_typed, 1);
        let _ = crate::method!(_surface_any, -1);
        let _ = Array::from_value_unchecked;
        let _ = Array::as_value;
        let _ = Array::as_raw;
        let _ = Array::push;
        let _ = Array::entry;
        let _ = Hash::from_value_unchecked;
        let _ = Hash::as_value;
        let _ = Hash::as_raw;
        let _ = Hash::set;
        let _ = Hash::get;
        let _ = Hash::keys;
        let _ = Ccontext::new;
        let _ = Ccontext::load_nstring;
        let _ = <i32 as IntoValue>::into_value;
        let _ = <f64 as IntoValue>::into_value;
        let _ = <bool as IntoValue>::into_value;
        let _ = <i32 as FromValue>::from_value;
        let _ = <f64 as FromValue>::from_value;
    }

    #[test]
    fn typed_mrb_func_t_coerces_from_value_bridge() {
        // Compile-time check (companion to beni-sys's
        // `mrb_func_t_is_a_valid_extern_c_fn_pointer`): building a
        // function with the typed `Value`-based signature must
        // coerce to `crate::mrb_func_t` without an explicit cast.
        // If `Value`'s `#[repr(transparent)]` over `mrb_value` ever
        // drifts (or someone removes the repr attribute), the
        // `transmute` inside `Module::define_method` becomes UB —
        // this test together with `value::tests::value_shares_abi_
        // with_mrb_value` is the guard rail.
        unsafe extern "C" fn _stub(_mrb: *mut sys::mrb_state, _self_: Value) -> Value {
            Value::zeroed()
        }
        let _f: crate::mrb_func_t = _stub;
    }
}
