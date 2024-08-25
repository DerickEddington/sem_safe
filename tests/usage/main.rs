#![cfg(test)] // Suppress `clippy::tests_outside_test_module` lint.
#![allow(
    clippy::assertions_on_result_states,
    clippy::unwrap_used,
    unused_results,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use std::{ffi::CString, process};

#[path = "../help/util.rs"]
mod util;
use util::*;

#[allow(dead_code)]
pub(crate) fn name(sub: &str) -> CString {
    CString::new(format!("/testing-sem_safe-usage-{}-{sub}", process::id())).unwrap()
}


mod refs;

#[cfg(all(feature = "unnamed", not(target_os = "macos")))]
mod unnamed;

#[cfg(feature = "named")]
mod named;

#[cfg(feature = "anonymous")]
mod anonymous;

#[cfg(feature = "plaster")]
mod plaster;
