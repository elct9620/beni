//! The typed wrapper's test suite, run from consumer position.
//!
//! Carries no library surface of its own — each module below is a
//! `#[cfg(test)]` suite reaching `beni` through its public paths.

#[cfg(test)]
mod smoke_test;
