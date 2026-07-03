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
//! L1  RAII / newtypes  state         (Mrb owning *mut mrb_state,
//!                                     ArenaScope arena bracketing)
//!                      value         (Value newtype + cstr! / cstr_ptr)
//!                      class         (RClass / RModule handles + traits)
//!                      array / hash  (typed factories on top of Value)
//!                      symbol        (Symbol newtype + intern / name)
//!                      data          (DataType<T> + CDATA wrap / get)
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
pub mod data;
pub mod error;
pub mod gem;
pub mod hash;
pub mod method;
pub mod proc;
pub mod range;
pub mod state;
pub mod string;
pub mod symbol;
pub mod value;

pub use state::arena::ArenaScope;
pub use state::{Mrb, MrbOpenError};

pub use state::args::{format, Format};

pub use ccontext::Ccontext;

pub use array::Array;
pub use class::{Module, Object, RClass, RModule};
pub use convert::{FromValue, IntoValue};
pub use data::DataType;
pub use error::Error;
pub use gem::Gem;
pub use hash::{ForEach, Hash};
pub use method::{MethodDef, MethodReturn};
pub use proc::Proc;
pub use range::{Range, RangeBegLen};
pub use string::RString;
pub use symbol::{IntoSym, Symbol};
pub use value::cstr_ptr;
pub use value::{Break, Value};

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
/// alias to `sys::mrb_func_t` happens once, inside the registration
/// plumbing the `Module` / `Object` traits share.
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
        // placeholder build before any consumer sees it. The
        // reference list is enforced by `rake api:surface`, which
        // scans the crate's inherent pub fns and fails on drift in
        // either direction.
        let _ = Mrb::open;
        let _ = Mrb::as_ptr;
        let _ = Mrb::borrow_raw;
        let _ = Mrb::pending_exc;
        let _ = Mrb::set_pending_exc;
        let _ = Mrb::clear_exc;
        let _ = Mrb::object_class;
        let _ = Mrb::define_module::<&core::ffi::CStr>;
        let _ = Mrb::define_class::<&core::ffi::CStr>;
        let _ = Mrb::class_get::<&core::ffi::CStr>;
        let _ = Mrb::module_get::<&core::ffi::CStr>;
        let _ = Mrb::define_global_const;
        let _ = Mrb::gv_set;
        let _ = Mrb::gv_get;
        let _ = Mrb::arena_scope;
        let _ = ArenaScope::keep;
        let _ = Mrb::str_new;
        let _ = Mrb::str_new_cstr;
        let _ = Mrb::str_new_capa;
        let _ = Mrb::ary_new;
        let _ = Mrb::hash_new;
        let _ = Mrb::range_new;
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
        let _ = <format::S as Format>::read;
        let _ = <format::Str as Format>::read;
        let _ = <format::RestBlock as Format>::read;
        let _ = Value::nil;
        let _ = Value::true_;
        let _ = Value::false_;
        let _ = Value::from_int;
        let _ = Value::from_float;
        let _ = Value::obj_as_string;
        let _ = Value::any_to_s;
        let _ = Value::obj_dup;
        let _ = Value::obj_clone;
        let _ = Value::classname;
        let _ = Value::to_string;
        let _ = Value::as_class_ptr;
        let _ = Value::funcall::<&core::ffi::CStr>;
        let _ = Value::funcall_argv;
        let _ = Value::is_nil;
        let _ = Value::is_integer;
        let _ = Value::is_float;
        let _ = Value::is_array;
        let _ = Value::is_hash;
        let _ = Value::is_class;
        let _ = Value::is_proc;
        let _ = Value::is_data;
        let _ = Value::is_string;
        let _ = Value::is_symbol;
        let _ = Value::data_get::<i32>;
        let _ = Value::data_reinit::<i32>;
        let _ = Value::unbox_integer;
        let _ = Value::unbox_float;
        let _ = Value::ary_entry;
        let _ = Value::iv_set;
        let _ = Value::iv_get;
        let _ = Value::iv_defined;
        let _ = Value::const_defined;
        let _ = Value::const_get;
        let _ = Value::cv_get;
        let _ = Value::respond_to;
        let _ = Value::as_break;
        let _ = Break::value;
        let _ = RClass::real;
        let _ = RClass::to_value;
        let _ = RClass::obj_new;
        let _ = RClass::raise;
        let _ = RClass::exc_new;
        let _ = RClass::exc_new_str;
        let _ = RClass::is_null;
        let _ = RClass::set_instance_data_tt;
        let _ = RClass::data_wrap::<i32>;
        let _ = DataType::<i32>::new;
        let _ = RModule::from_raw;
        let _ = RModule::as_raw;
        let _ = <RClass as Module>::define_class::<&core::ffi::CStr>;
        let _ = <RClass as Module>::define_module::<&core::ffi::CStr>;
        let _ = <RClass as Module>::class_get::<&core::ffi::CStr>;
        let _ = <RClass as Module>::module_get::<&core::ffi::CStr>;
        let _ = <RClass as Module>::define_method::<&core::ffi::CStr>;
        let _ = <RClass as Module>::define_private_method::<&core::ffi::CStr>;
        let _ = <RClass as Module>::define_module_function::<&core::ffi::CStr>;
        let _ = <RClass as Module>::define_const::<&core::ffi::CStr>;
        let _ = <RClass as Module>::name;
        let _ = <RModule as Module>::define_class::<&core::ffi::CStr>;
        let _ = <RModule as Module>::define_method::<&core::ffi::CStr>;
        let _ = <RModule as Module>::define_private_method::<&core::ffi::CStr>;
        let _ = <RModule as Module>::define_module_function::<&core::ffi::CStr>;
        let _ = <RModule as Module>::define_const::<&core::ffi::CStr>;
        let _ = <RClass as Object>::define_singleton_method::<&core::ffi::CStr>;
        let _ = <RModule as Object>::define_singleton_method::<&core::ffi::CStr>;
        let _ = Error::message;
        let _ = Error::argnum;
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
        let _ = Array::len;
        let _ = Array::is_empty;
        let _ = RString::from_value_unchecked;
        let _ = RString::as_value;
        let _ = RString::as_raw;
        let _ = RString::cat;
        let _ = RString::cat_str;
        let _ = RString::as_bytes;
        let _ = RString::to_bytes;
        let _ = RString::len;
        let _ = RString::is_empty;
        let _ = Proc::from_value_unchecked;
        let _ = Proc::as_value;
        let _ = Proc::as_raw;
        let _ = Proc::call;
        let _ = Hash::from_value_unchecked;
        let _ = Hash::as_value;
        let _ = Hash::as_raw;
        let _ = Hash::set;
        let _ = Hash::get;
        let _ = Hash::keys;
        let _ = Range::from_value_unchecked;
        let _ = Range::as_value;
        let _ = Range::as_raw;
        let _ = Range::begin;
        let _ = Range::end_;
        let _ = Range::is_exclusive;
        let _ = Range::beg_len;
        let _ = Symbol::from_value_unchecked;
        let _ = Symbol::as_value;
        let _ = Symbol::as_raw;
        let _ = Symbol::new;
        let _ = Symbol::from_sym;
        let _ = Symbol::to_sym;
        let _ = Symbol::name;
        let _ = <&core::ffi::CStr as IntoSym>::into_sym;
        let _ = <Symbol as IntoSym>::into_sym;
        let _ = Ccontext::new;
        let _ = Ccontext::load_nstring;
        let _ = <i32 as IntoValue>::into_value;
        let _ = <f64 as IntoValue>::into_value;
        let _ = <bool as IntoValue>::into_value;
        let _ = <i32 as FromValue>::from_value;
        let _ = <f64 as FromValue>::from_value;
        let _ = <Array as FromValue>::from_value;
        let _ = <Hash as FromValue>::from_value;
        let _ = <Range as FromValue>::from_value;
        let _ = <RClass as FromValue>::from_value;
        let _ = <Proc as FromValue>::from_value;
        let _ = <Symbol as FromValue>::from_value;
        let _ = <Symbol as IntoValue>::into_value;
        let _ = Array::clear;
        let _ = Array::concat;
        let _ = Array::dup;
        let _ = Array::join;
        let _ = Array::pop;
        let _ = Array::replace;
        let _ = Array::resize;
        let _ = Array::shift;
        let _ = Array::splice;
        let _ = Array::store;
        let _ = Array::unshift;
        let _ = DataType::<u8>::new;
        let _ = Error::new;
        let _ = Hash::clear;
        let _ = Hash::contains_key;
        let _ = Hash::delete;
        let _ = Hash::dup;
        let _ = Hash::each::<fn(Value, Value) -> ForEach>;
        let _ = Hash::fetch;
        let _ = Hash::is_empty;
        let _ = Hash::len;
        let _ = Hash::update;
        let _ = Hash::values;
        let _ = MethodDef::new_with_block;
        let _ = MethodDef::new_with_opt;
        let _ = Mrb::arg1;
        let _ = Mrb::argc;
        let _ = Mrb::argv;
        let _ = Mrb::ary_new_capa;
        let _ = Mrb::ary_new_from_values;
        let _ = Mrb::assoc_new;
        let _ = Mrb::block_given;
        let _ = Mrb::class_defined::<&core::ffi::CStr>;
        let _ = Mrb::class_new;
        let _ = Mrb::exc_get::<&core::ffi::CStr>;
        let _ = Mrb::full_gc;
        let _ = Mrb::gv_remove;
        let _ = Mrb::hash_new_capa;
        let _ = Mrb::incremental_gc;
        let _ = Mrb::intern;
        let _ = Mrb::intern_check;
        let _ = Mrb::intern_static;
        let _ = Mrb::load_string;
        let _ = Mrb::module_new;
        let _ = Mrb::rescue::<fn(&Mrb) -> Value, fn(&Mrb, Value) -> Value>;
        let _ = Mrb::str_new_static;
        let _ = Mrb::sym_dump;
        let _ = Mrb::sym_name_len;
        let _ = RClass::as_raw;
        let _ = RClass::from_raw;
        let _ = RString::cat_cstr;
        let _ = RString::cmp;
        let _ = RString::concat;
        let _ = RString::dup;
        let _ = RString::eq;
        let _ = RString::index;
        let _ = RString::intern;
        let _ = RString::plus;
        let _ = RString::resize;
        let _ = RString::substr;
        let _ = RString::to_cstr;
        let _ = RString::to_f;
        let _ = RString::to_i;
        let _ = RString::to_inum;
        let _ = Symbol::dump;
        let _ = Symbol::name_bytes;
        let _ = Symbol::to_str;
        let _ = Value::add;
        let _ = Value::as_float;
        let _ = Value::as_int;
        let _ = Value::as_raw;
        let _ = Value::check_frozen;
        let _ = Value::class;
        let _ = Value::cmp;
        let _ = Value::const_defined_at;
        let _ = Value::const_remove;
        let _ = Value::const_set;
        let _ = Value::cv_defined;
        let _ = Value::cv_set;
        let _ = Value::each_iv::<fn(Symbol, Value) -> ForEach>;
        let _ = Value::ensure_array;
        let _ = Value::ensure_float;
        let _ = Value::ensure_hash;
        let _ = Value::ensure_int;
        let _ = Value::ensure_string;
        let _ = Value::eql;
        let _ = Value::equal;
        let _ = Value::float_to_int;
        let _ = Value::freeze;
        let _ = Value::from_raw;
        let _ = Value::funcall_with_block::<&core::ffi::CStr>;
        let _ = Value::inspect;
        let _ = Value::int_to_str;
        let _ = Value::into_raw;
        let _ = Value::is_exception;
        let _ = Value::is_false;
        let _ = Value::is_instance_of;
        let _ = Value::is_kind_of;
        let _ = Value::is_module;
        let _ = Value::is_range;
        let _ = Value::is_true;
        let _ = Value::iv_remove;
        let _ = Value::mul;
        let _ = Value::obj_equal;
        let _ = Value::object_id;
        let _ = Value::singleton_class;
        let _ = Value::sub;
        let _ = Value::to_ary;
        let _ = Value::to_bool;
        let _ = Value::to_sym;
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
