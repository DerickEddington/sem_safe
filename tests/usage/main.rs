//! Test usage of the various aspects.

#![cfg(test)] // Suppress `clippy::tests_outside_test_module` lint.
#![allow(
    clippy::assertions_on_result_states,
    clippy::unwrap_used,
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc,
    unused_results,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

#[path = "../help/util.rs"]
mod util;
use util::*;


mod refs;

#[cfg(all(feature = "unnamed", not(target_os = "macos")))]
mod unnamed;

#[cfg(feature = "named")]
mod named;

#[cfg(feature = "anonymous")]
mod anonymous;

#[cfg(feature = "plaster")]
mod plaster;
