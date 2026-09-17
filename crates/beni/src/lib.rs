//! beni — typed Rust wrapper over the `beni-sys` FFI surface.
//!
//! This crate owns every Rust-level abstraction above the mruby C
//! API: the `Mrb` / `Ccontext` RAII types, the `Value` / `RClass` /
//! `Array` / `Hash` newtypes, the `IntoValue` / `FromValue` trait
//! seam, and the `Format`-based `mrb_get_args` dispatch. The sibling
//! `beni-sys` crate keeps
//! only the bindgen-generated `extern "C"` declarations and the
//! layout-safe C shims — the same split magnus + rb-sys apply at
//! the CRuby boundary.
//!
//! ## Layering
//!
//! ```text
//! L2  trait seams      convert        (IntoValue / FromValue)
//!                      state::args    (Format trait + ZST + GAT dispatch)
//!                      method         (method! bridges + MethodN crossing)
//!                      gem            (Gem trait + Mrb::init_gem)
//!
//! L1  RAII / newtypes  state          (Mrb owning *mut mrb_state,
//!                                      ArenaScope arena bracketing)
//!                      value          (Value newtype + cstr! / cstr_ptr)
//!                      class          (RClass / RModule / ExceptionClass
//!                                      handles + traits)
//!                      array / hash   (typed factories on top of Value)
//!                      string / range (RString / Range newtypes)
//!                      symbol / proc  (Symbol / Proc newtypes)
//!                      data           (DataType<T> + CDATA wrap / get)
//!                      ccontext       (Ccontext RAII)
//!                      error / parse  (Error + ParseMessage — the shapes
//!                                      a failure is reported in)
//!
//! L0  raw FFI          sys          (beni-sys::* + protect / catch_unwind)
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
//! `beni::sys` carries every `beni-sys` binding (`sys::mrb_value`,
//! `sys::mrb_state`, `sys::mrb_func_t`, …) under a short import path,
//! together with `sys::protect`, which catches a raw binding's raise as
//! an `Err`, and `sys::catch_unwind`, which does the same for a panic in
//! a C callback — the counterpart of magnus's `rb_sys` module.

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
pub mod sys;
pub mod value;

pub use state::arena::ArenaScope;
pub use state::root::GcRoot;
pub use state::{Mrb, MrbOpenError};

pub use state::args::{format, Format};

#[cfg(feature = "compiler")]
pub use ccontext::Ccontext;

pub use array::Array;
pub use class::{ExceptionClass, Module, Object, RClass, RModule};
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

/// The build script's integer-width read, reachable here because a build
/// script is outside `cargo test`'s reach.
#[cfg(test)]
mod width {
    include!("../build/width.rs");
}
