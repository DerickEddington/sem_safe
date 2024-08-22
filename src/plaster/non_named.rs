//! A "non-named" semaphore abstraction that is an unnamed single-process-private semaphore on all
//! OSs except macOS (and Mac OS X) where it has to be an anonymous "named" semaphore.

#[cfg(all(target_os = "macos", not(feature = "anonymous")))]
core::compile_error!("The \"plaster\" feature on MacOS needs the \"anonymous\" feature.");

#[cfg(all(not(target_os = "macos"), not(feature = "unnamed")))]
core::compile_error!("The \"plaster\" feature on non-MacOS needs the \"unnamed\" feature.");


#[cfg(all(target_os = "macos", feature = "anonymous"))]
/// The platform is Mac, so use our [`anonymous::Semaphore`](crate::anonymous::Semaphore).
pub type Semaphore = crate::anonymous::Semaphore;

#[cfg(all(not(target_os = "macos"), feature = "unnamed"))]
/// The platform is not Mac, so use the [`unnamed::Semaphore`](crate::unnamed::Semaphore).
pub type Semaphore = crate::unnamed::Semaphore;
