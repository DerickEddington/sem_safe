#![doc = include_str!("../README.md")]
#![no_std]
#![allow(clippy::result_unit_err)]
#[cfg(not(unix))]
core::compile_error!("Only supported on POSIX.");
#[cfg(not(any(feature = "unnamed", feature = "named")))]
core::compile_error!("Must enable at least one of the kinds of semaphore.");


pub use refs::*;
mod refs;

#[cfg(feature = "unnamed")]
pub mod unnamed;

#[cfg(feature = "named")]
pub mod named;

#[cfg(feature = "anonymous")]
pub mod anonymous;

pub(crate) use init_once::InitOnce;
mod init_once;
