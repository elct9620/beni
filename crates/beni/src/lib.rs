//! beni — typed Rust wrapper over the `beni-sys` FFI surface.
//!
//! This crate owns every Rust-level abstraction above the mruby C
//! API: the `Mrb` / `Ccontext` RAII types, the `Value` / `RClass` /
//! `RModule` / `RArray` / `RHash` handles, the `IntoValue` / `FromValue`
//! / `TryConvert` conversion seam, and the `scan_args` reads of a
//! call's arguments. The sibling `beni-sys` crate keeps only the
//! bindgen-generated `extern "C"` declarations and the layout-safe C
//! shims — the same split magnus + rb-sys apply at the CRuby boundary.
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
//!                      value          (Value + ReprValue, the trait
//!                                      every handle stands for one by)
//!                      class          (RClass / RModule / ExceptionClass
//!                                      handles)
//!                      module / object (Module and Object traits)
//!                      array / hash   (RArray / RHash handles)
//!                      string / range (RString / Range handles)
//!                      symbol / proc  (Id, Symbol / Proc handles)
//!                      tagged         (handles a value converts into
//!                                      by its type tag alone)
//!                      data           (DataType<T>, a carrier's data type)
//!                      typed_data     (TypedData + RTypedData / Obj<T>)
//!                      inline_struct  (InlineStruct + Inline<T>)
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

pub mod array;
#[cfg(feature = "compiler")]
pub mod ccontext;
pub mod class;
pub mod convert;
pub mod data;
pub mod error;
pub mod gem;
pub mod hash;
pub mod inline_struct;
pub mod method;
pub mod module;
mod object;
pub mod parse;
pub mod proc;
pub mod range;
pub mod scan_args;
pub mod state;
pub mod string;
pub mod symbol;
pub mod sys;
pub mod tagged;
pub mod try_convert;
pub mod typed_data;
pub mod value;

/// The traits whose methods the typed handles are used through, imported
/// anonymously — magnus's `prelude`: `use beni::prelude::*;`.
pub mod prelude {
    pub use crate::{FromValue as _, Module as _, Object as _, ReprValue as _};
}

pub use state::arena::ArenaScope;
pub use state::root::GcRoot;
pub use state::{Mrb, MrbOpenError};

#[cfg(feature = "compiler")]
pub use ccontext::Ccontext;

pub use array::RArray;
pub use class::{ExceptionClass, RClass, RModule};
pub use convert::{FromValue, IntoValue};
pub use data::DataType;
pub use error::Error;
pub use gem::Gem;
pub use hash::{ForEach, RHash};
pub use inline_struct::{Inline, InlineStruct, InlineType};
pub use method::{MethodDef, ReturnValue};
pub use module::Module;
pub use object::Object;
pub use parse::ParseMessage;
pub use proc::{DumpOptions, Proc};
pub use range::{Range, RangeBegLen};
pub use string::RString;
pub use symbol::{Id, IntoId, Symbol};
pub use tagged::{
    Exception, Fiber, Float, Integer, Qfalse, Qnil, Qtrue, Qundef, RComplex, RCptr, RInlineStruct,
    RObject, RRational, RSet, RStruct,
};
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
/// a nested class, and a path holding an empty segment is a compile
/// error. `name` names the data type and defaults to `class`.
///
/// `TypedData::mark_carriers` resolves the path, marks the class to
/// carry data with its default allocator undefined, and holds it in the
/// interpreter's carrier record; it answers an `Err` for a path naming
/// no class or a class refusing the mark. Every later naming reads that
/// record, so what a Ruby program binds over the path reaches no wrap.
/// Call it from the gem's `init`, for each interpreter, before any Ruby
/// program runs — naming a class the record does not hold panics.
///
/// ```
/// # use beni::{Error, Mrb, TypedData};
/// # #[beni::wrap(class = "Point")]
/// # struct Point { x: i32 }
/// fn init(mrb: &Mrb) -> Result<(), Error> {
///     mrb.define_class(c"Point", mrb.object_class())?;
///     Point::mark_carriers(mrb)
/// }
/// ```
///
/// mruby hands a class its superclass's mark and allocator state when
/// the class is defined, so a subclass Ruby defines before the type's
/// carriers are marked carries neither, and wrapping into it panics.
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
///
/// ```compile_fail
/// #[beni::wrap(class = "Geometry::")]
/// struct Point;
/// ```
///
/// ```compile_fail
/// #[beni::wrap(class = "Point", name = "Po\0int")]
/// struct Point;
/// ```
///
/// With `inline`, the struct is stored inside the object as an inline
/// struct instead: `wrap` derives `InlineStruct`, which needs a
/// `bytemuck::Pod` type. Only `wrap` takes `inline`:
///
/// ```
/// #[beni::wrap(class = "Vector2D", inline)]
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// #[repr(C)]
/// struct Vector2D {
///     x: f64,
///     y: f64,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(beni::TypedData)]
/// #[beni(class = "Point", inline)]
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
///
/// ```compile_fail
/// #[derive(beni::TypedData)]
/// struct Shape;
/// ```
///
/// ```compile_fail
/// #[derive(beni::TypedData)]
/// #[beni(class = "Shape")]
/// #[beni(name = "Shape")]
/// struct Shape;
/// ```
pub use beni_macros::TypedData;

/// ```
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
/// #[beni(class = "Vector2D")]
/// #[repr(C)]
/// struct Vector2D {
///     x: f64,
///     y: f64,
/// }
/// ```
///
/// Implements `InlineStruct` and the by-value `TryConvert` / `IntoValue`
/// for a plain-data struct; `class` and `name` read as `wrap` reads them.
/// A type that is not `bytemuck::Pod`, outgrows three pointer widths, is
/// not a struct, or is generic does not compile, nor does any attribute
/// beyond `class` and `name`:
///
/// ```compile_fail
/// #[derive(beni::InlineStruct)]
/// #[beni(class = "Plain")]
/// struct Plain(u32);
/// ```
///
/// ```compile_fail
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
/// #[beni(class = "Wide")]
/// #[repr(C)]
/// struct Wide([usize; 4]);
/// ```
///
/// ```compile_fail
/// #[derive(Clone, Copy, beni::InlineStruct)]
/// #[beni(class = "Turn")]
/// #[repr(u32)]
/// enum Turn { Left, Right }
/// ```
///
/// ```compile_fail
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
/// #[beni(class = "Pair")]
/// #[repr(C)]
/// struct Pair<T: bytemuck::Pod>(T, T);
/// ```
///
/// ```compile_fail
/// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, beni::InlineStruct)]
/// #[beni(class = "Cell", inline)]
/// #[repr(C)]
/// struct Cell(u32);
/// ```
pub use beni_macros::InlineStruct;
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

/// The README's example, compiled and run as a doctest so the page
/// crates.io shows keeps to the current surface.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
