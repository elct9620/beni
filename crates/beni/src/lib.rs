//! beni — typed Rust wrapper over the `beni-sys` FFI surface.
//!
//! This crate owns every Rust-level abstraction above the mruby C
//! API: the `Mrb` / `Ccontext` RAII types, the `Value` / `RClass` /
//! `Array` / `Hash` newtypes, the `IntoValue` / `FromValue` /
//! `TryConvert` trait seam, and the `scan_args` reads of a call's arguments. The sibling
//! `beni-sys` crate keeps
//! only the bindgen-generated `extern "C"` declarations and the
//! layout-safe C shims — the same split magnus + rb-sys apply at
//! the CRuby boundary.
//!
//! ## Layering
//!
//! ```text
//! L2  trait seams      convert        (IntoValue / FromValue)
//!                      try_convert    (TryConvert, the argument crossing)
//!                      scan_args      (magnus-shaped frame reads)
//!                      method         (method! bridges + MethodN crossing)
//!                      gem            (Gem trait + Mrb::init_gem)
//!                      wrap / TypedData derive (beni-macros,
//!                                      re-exported at the root)
//!
//! L1  RAII / newtypes  state          (Mrb owning *mut mrb_state,
//!                                      ArenaScope arena bracketing)
//!                      value          (Value newtype + cstr! / cstr_ptr)
//!                      class          (RClass / RModule / ExceptionClass
//!                                      handles + traits)
//!                      array / hash   (typed factories on top of Value)
//!                      string / range (RString / Range newtypes)
//!                      symbol / proc  (Id, Symbol / Proc newtypes)
//!                      data           (DataType<T>, a carrier's data type)
//!                      typed_data     (TypedData + RTypedData / Obj<T>)
//!                      ccontext       (Ccontext RAII)
//!                      error / parse  (Error + ParseMessage — the shapes
//!                                      a failure is reported in)
//!
//! L0  raw FFI          sys          (beni-sys::* + protect / catch_unwind)
//! ```
//!
//! ## Cargo features
//!
//! `compiler`, on by default, carries what mruby keeps in its compiler
//! gem: the `Ccontext` compile context and `Mrb::load_string`. Turn
//! default features off to embed mruby without compiling Ruby at run
//! time — loading precompiled bytecode needs no compiler and stays.
//!
//! `bytes`, off by default, carries `TryConvert` and `IntoValue` for
//! `bytes::Bytes`, as magnus's `bytes` feature does.
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

/// The traits whose methods the typed handles are used through, imported
/// anonymously — magnus's `prelude`: `use beni::prelude::*;`.
pub mod prelude {
    pub use crate::{FromValue as _, Module as _, Object as _, ReprValue as _};
}
pub mod range;
pub mod scan_args;
pub mod state;
pub mod string;
pub mod symbol;
pub mod sys;
pub mod try_convert;
pub mod typed_data;
pub mod value;

pub use state::arena::ArenaScope;
pub use state::root::GcRoot;
pub use state::{Mrb, MrbOpenError};

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
pub use symbol::{Id, IntoId, Symbol};
pub use try_convert::TryConvert;
pub use typed_data::{RTypedData, TypedData};

/// ```
/// #[beni::wrap(class = "Point")]
/// struct Point {
///     x: i32,
///     y: i32,
/// }
/// ```
///
/// `class` is a constant path from `Object` — `"Geometry::Point"` names
/// a nested class — resolved in the interpreter at hand each time the
/// type names it, and marked to carry data with its default allocator
/// undefined as it is. A path naming no class, or a class that refuses
/// the mark, panics. `name` names the data type and defaults to `class`.
///
/// mruby hands a class its superclass's mark and allocator state when
/// the class is defined, so a subclass Ruby defines before the type
/// first names its class carries neither, and wrapping into it panics.
/// Call `TypedData::class` before Ruby code subclasses the type's class.
///
/// The attributes magnus accepts beyond `class` and `name` are GC and
/// Ractor hints mruby's data type has no counterpart for, and are
/// compile errors, as is a generic type:
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", mark)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", size)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", compact)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", free_immediately)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", wb_protected)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", frozen_shareable)]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", unsafe_generics)]
/// struct Point<T: Send + 'static>(T);
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point")]
/// struct Point<T: Send + 'static>(T);
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point")]
/// struct Point {
///     #[beni(opaque_attr_reader)]
///     x: i32,
/// }
/// ```
///
/// ```compile_fail
/// #[beni::wrap(name = "Point")]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Po\0int")]
/// struct Point;
/// ```
pub use beni_macros::wrap;

/// ```
/// #[derive(beni::TypedData)]
/// #[beni(class = "Shape")]
/// enum Shape {
///     #[beni(class = "Shape::Circle")]
///     Circle { r: f64 },
///     Square(f64),
/// }
/// ```
///
/// Each variant carrying `#[beni(class = "...")]` wraps as that class —
/// the type's class or a subclass of it — and every other variant as
/// the type's class. See `wrap` for the attributes and what naming a
/// class does.
///
/// ```compile_fail
/// #[derive(beni::TypedData)]
/// #[beni(class = "Shape")]
/// enum Shape {
///     #[beni(class = "Circle", mark)]
///     Circle,
/// }
/// ```
pub use beni_macros::TypedData;
pub use value::cstr_ptr;
pub use value::{Break, ReprValue, Value};

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
