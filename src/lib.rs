#![doc = include_str!("../README.md")]
#![no_std]
#![allow(clippy::result_unit_err)]
// Warn about this one but avoid annoying hits for dev-dependencies.
#![cfg_attr(test, allow(unused_crate_dependencies))]

#[cfg(not(unix))]
core::compile_error!("Only supported on POSIX.");
#[cfg(all(target_os = "macos", not(feature = "named"), feature = "unnamed"))]
core::compile_error!("MacOS doesn't support the \"unnamed\" feature.");
#[cfg(not(any(feature = "unnamed", feature = "named")))]
core::compile_error!("Must enable at least one of the kinds of semaphore.");


pub use refs::*;
mod refs;

#[cfg(all(feature = "unnamed", not(target_os = "macos")))]
pub mod unnamed;

#[cfg(any(feature = "unnamed", feature = "anonymous"))]
pub mod non_named;

#[cfg(feature = "named")]
pub mod named;

#[cfg(feature = "anonymous")]
pub mod anonymous;

#[cfg(feature = "plaster")]
pub mod plaster;
