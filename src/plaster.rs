//! Plaster-over the lack of the unnamed semaphores on some OSs (i.e. macOS), by providing uniform
//! "non-named" semaphore abstractions across all OSs.

pub mod non_named;
