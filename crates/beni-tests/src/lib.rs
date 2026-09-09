//! The typed wrapper's test suite, run from consumer position.
//!
//! Carries no library surface of its own — the whole crate is the
//! suite, so the gate sits here rather than on each module below.
#![cfg(test)]

mod support;

mod arena_test;
mod args_test;
mod array_test;
mod ccontext_test;
mod class_test;
mod convert_test;
mod data_test;
mod define_test;
mod error_test;
mod factory_test;
mod gem_test;
mod hash_test;
mod load_test;
mod method_test;
mod proc_test;
mod protect_test;
mod range_test;
mod root_test;
mod smoke_test;
mod state_symbol_test;
mod state_test;
mod string_test;
mod surface_test;
mod symbol_test;
mod value_test;
