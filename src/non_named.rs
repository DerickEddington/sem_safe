//! Aspects common to all "non-named" semaphore abstractions.

pub(crate) use init_once::InitOnce;
mod init_once;

use crate::SemaphoreRef;
use core::{ffi::c_uint,
           fmt,
           fmt::{Display, Formatter},
           hint,
           pin::Pin};


/// Methods and super-trait bounds common to all "non-named" semaphore abstractions.
///
/// This must be `Sync` because major purposes of this API are: that multiple threads may share
/// use of an instance in order to indirectly modify the semaphore, and that the `Self` may be
/// used in `static`s.
///
/// This is `Default` so that generic uses can create instances.  The `Default` impl must create
/// the underlying semaphore as uninitialized.
#[allow(private_bounds)]
pub trait Semaphore: Default + Sync + Sealed {
    /// Initialize the underlying OS semaphore and return a [`SemaphoreRef`] to it.
    ///
    /// Usually this should only be called once.  But this guards against multiple calls on the
    /// same instance (perhaps by multiple threads), to ensure the initialization is only done
    /// once.
    ///
    /// # Errors
    /// Returns `Err(true)` if the initialization was already successfully done, or is being done,
    /// by another call (perhaps by another thread).  Returns `Err(false)` if the call tried to do
    /// the initialization but there was an error with that, in which case `errno` is set to
    /// indicate the error.
    fn init_with(
        self: Pin<&Self>,
        is_shared: bool,
        sem_count: c_uint,
    ) -> Result<SemaphoreRef<'_>, bool>;

    /// Get a [`SemaphoreRef`] to `self`, so that semaphore operations can be done on `self`.
    ///
    /// This method is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    ///
    /// # Errors
    /// If `self` was not previously initialized.
    fn sem_ref(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, ()>;

    /// Like [`Self::init_with`] but uses `is_shared = false` and `sem_count = 0`.
    ///
    /// This is a common use-case to have a semaphore that is private to the calling process
    /// (i.e. not shared between multiple processes, unless by `fork()`) and that starts with a
    /// "resource count" of zero so that initial waiting on it blocks waiter threads until a post
    /// indicates to wake.
    ///
    /// # Errors
    /// Same as [`Self::init_with`].
    #[inline]
    fn init(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, bool> { self.init_with(false, 0) }

    /// Like [`Self::try_init_with`] but uses `is_shared = false` and `sem_count = 0`, similar to
    /// [`Self::init`].
    #[must_use]
    #[inline]
    fn try_init(self: Pin<&Self>, limit: u64) -> Option<SemaphoreRef<'_>> {
        self.try_init_with(limit, false, 0)
    }

    /// Try to initialize `self`, repeatedly if necessary, if not already initialized, and return
    /// a reference to it.
    ///
    /// Will spin-loop waiting until it's initialized, up to the given `limit` of retries.
    #[must_use]
    #[inline]
    fn try_init_with(
        self: Pin<&Self>,
        mut limit: u64,
        is_shared: bool,
        sem_count: c_uint,
    ) -> Option<SemaphoreRef<'_>> {
        match self.init_with(is_shared, sem_count) {
            Ok(sem_ref) => Some(sem_ref),
            Err(true) => loop {
                // It was already initialized or another thread was in the middle of initializing
                // it.
                if let Ok(sem_ref) = self.sem_ref() {
                    break Some(sem_ref); // Initialization ready.
                }
                // Not yet initialized by the other thread.
                limit = limit.saturating_sub(1);
                if limit == 0 {
                    break None; // Waited too long. Something is wrong, probably failed.
                }
                hint::spin_loop();
            },
            Err(false) => None, // Initialization failed.
        }
    }

    /// Return a value that displays `self`.
    ///
    /// Shows the current count value only if the semaphore has been initialized.
    ///
    /// (This is needed because `impl Display for Self` wouldn't work.)
    #[must_use]
    #[inline]
    fn display(self: Pin<&Self>) -> impl Display + '_ { Displayer(self) }
}


struct Displayer<T>(T);

impl<T: Semaphore + ?Sized> Display for Displayer<Pin<&T>> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0.sem_ref() {
            Ok(sem) => <SemaphoreRef<'_> as Display>::fmt(&sem, f),
            Err(()) => write!(f, "<Semaphore>"),
        }
    }
}


pub(crate) trait Sealed {}
