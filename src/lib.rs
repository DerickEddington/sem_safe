#![doc = include_str!("../README.md")]
#![no_std]
#![allow(clippy::result_unit_err)]
#[cfg(not(unix))]
core::compile_error!("Only supported on POSIX.");


pub use refs::*;
mod refs;

pub mod unnamed;
