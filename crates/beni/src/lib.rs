//! Reserved crate name for the typed mruby binding (magnus analog),
//! under active development at <https://github.com/elct9620/beni>.
//!
//! The typed wrapper is being extracted from the kobako project; this
//! version only re-exports the `beni-sys` placeholder as `sys`, the
//! raw-FFI escape hatch the final layout will keep.

pub use beni_sys as sys;
