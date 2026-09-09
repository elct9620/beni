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
//! L2  trait seams      convert        (IntoValue / FromValue)
//!                      state::args    (Format trait + ZST + GAT dispatch)
//!                      state::protect (closure-based mrb_protect_error)
//!                      method         (method! bridges + MethodN crossing)
//!                      gem            (Gem trait + Mrb::init_gem)
//!
//! L1  RAII / newtypes  state          (Mrb owning *mut mrb_state,
//!                                      ArenaScope arena bracketing)
//!                      value          (Value newtype + cstr! / cstr_ptr)
//!                      class          (RClass / RModule handles + traits)
//!                      array / hash   (typed factories on top of Value)
//!                      string / range (RString / Range newtypes)
//!                      symbol / proc  (Symbol / Proc newtypes)
//!                      data           (DataType<T> + CDATA wrap / get)
//!                      ccontext       (Ccontext RAII)
//!                      error / parse  (Error + ParseMessage — the shapes
//!                                      a failure is reported in)
//!
//! L0  raw FFI          beni-sys::*  (bindgen output + ABI constants)
//! ```
//!
//! ## Capability features
//!
//! `compiler`, on by default, carries what mruby keeps in its compiler
//! gem: the `Ccontext` compile context and `Mrb::load_string`. Turn
//! default features off to embed mruby without compiling Ruby at run
//! time — loading precompiled bytecode needs no compiler and stays.
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

pub mod array;
#[cfg(feature = "compiler")]
pub mod ccontext;
pub mod class;
pub mod convert;
pub mod data;
pub mod error;
pub mod gem;
pub mod hash;
pub mod method;
pub mod parse;
pub mod proc;
pub mod range;
pub mod state;
pub mod string;
pub mod symbol;
pub mod value;

pub use state::arena::ArenaScope;
pub use state::root::GcRoot;
pub use state::{Mrb, MrbOpenError};

pub use state::args::{format, Format};

#[cfg(feature = "compiler")]
pub use ccontext::Ccontext;

pub use array::Array;
pub use class::{Module, Object, RClass, RModule};
pub use convert::{FromValue, IntoValue};
pub use data::DataType;
pub use error::Error;
pub use gem::Gem;
pub use hash::{ForEach, Hash};
pub use method::{MethodDef, MethodReturn};
pub use parse::ParseMessage;
pub use proc::{DumpOptions, Proc};
pub use range::{Range, RangeBegLen};
pub use string::RString;
pub use symbol::{IntoSym, Symbol};
pub use value::cstr_ptr;
pub use value::{Break, Value};

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
pub type mrb_func_t = unsafe extern "C" fn(mrb: *mut sys::mrb_state, self_: Value) -> Value;
