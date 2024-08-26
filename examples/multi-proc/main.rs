//! Exercise using semaphores shared between multiple processes.

#![allow(
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::panic,
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::std_instead_of_core,
    unused_crate_dependencies, // Ignore the lib crate's deps that are supplied here also.
    unused_results,
)]

use cfg_if::cfg_if;


// If the needed features aren't enabled, we still want this example to not cause build failures.
// This doesn't use `Cargo.toml`'s `required-features` because that's not flexible enough.

cfg_if! { if #[cfg(any(all(feature = "unnamed", not(any(target_os = "macos",
                                                        target_os = "openbsd"))),
                       feature = "anonymous"))] {
    mod enabled;

    fn main() { enabled::main(); }
}
else {
    fn main() {
        if cfg!(target_os = "openbsd") {
            panic!("need feature \"anonymous\"");
        } else {
            panic!("need either feature \"unnamed\" (possibly with \"named\") or \"anonymous\"");
        }
    }
} }
