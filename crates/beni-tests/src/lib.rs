//! The typed wrapper's test suite, run from consumer position.
//!
//! Carries no library surface of its own — each module below is a
//! `#[cfg(test)]` suite reaching `beni` through its public paths.

#[cfg(test)]
mod arena_test;
#[cfg(test)]
mod args_test;
#[cfg(test)]
mod array_test;
#[cfg(test)]
mod ccontext_test;
#[cfg(test)]
mod class_test;
#[cfg(test)]
mod convert_test;
#[cfg(test)]
mod data_test;
#[cfg(test)]
mod define_test;
#[cfg(test)]
mod error_test;
#[cfg(test)]
mod factory_test;
#[cfg(test)]
mod gem_test;
#[cfg(test)]
mod hash_test;
#[cfg(test)]
mod load_test;
#[cfg(test)]
mod method_test;
#[cfg(test)]
mod proc_test;
#[cfg(test)]
mod protect_test;
#[cfg(test)]
mod range_test;
#[cfg(test)]
mod root_test;
#[cfg(test)]
mod smoke_test;
#[cfg(test)]
mod state_symbol_test;
#[cfg(test)]
mod state_test;
#[cfg(test)]
mod string_test;
#[cfg(test)]
mod surface_test;
#[cfg(test)]
mod symbol_test;
#[cfg(test)]
mod value_test;
