//! Unnamed semaphores.

use crate::SemaphoreRef;
use core::{cell::UnsafeCell,
           ffi::{c_int, c_uint},
           fmt::{self, Display, Formatter},
           hint,
           marker::PhantomPinned,
           mem::MaybeUninit,
           pin::Pin,
           sync::atomic::{AtomicU8,
                          Ordering::{Acquire, Relaxed, Release}}};


/// An "unnamed" [`sem_t`](
/// https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html)
/// that can only be used safely.
///
/// This must remain pinned for and after [`Self::init_with()`], because it's not clear if moving
/// a `sem_t` value is permitted after it's been initialized with `sem_init()`.  Using this as a
/// `static` item (not as `mut`able) is a common way to achieve that (via [`Pin::static_ref`]).
/// Or, [`pin!`](core::pin::pin) can also work.
#[must_use]
#[derive(Debug)]
pub struct Semaphore {
    inner:   MaybeUninit<UnsafeCell<libc::sem_t>>,
    state:   AtomicU8,
    _pinned: PhantomPinned,
}


/// SAFETY: The POSIX Semaphores API intends for `sem_t` to be shared between threads and its
/// operations are thread-safe (similar to atomic types).  Therefore we can expose this in Rust as
/// having "interior mutability".
unsafe impl Sync for Semaphore {}

// Note: `Send` isn't impl'ed, because it's not clear if moving a `sem_t` value is permitted after
// it's been initialized with `sem_init`.


impl Semaphore {
    // These values are decided only internally.
    const UNINITIALIZED: u8 = 0;
    const PREPARING: u8 = 1;
    const READY: u8 = 2;
    // These values are decided by the `sem_init` documentation.
    const SINGLE_PROCESS_PRIVATE: c_int = 0;
    const MULTI_PROCESS_SHARED: c_int = 1;

    /// Create an uninitialized `sem_t`.
    ///
    /// The only operations that can be done with a new instance are to [initialize](Self::init)
    /// it (which first requires pinning it) or drop it.
    #[inline]
    pub const fn new() -> Self {
        Self {
            inner:   MaybeUninit::uninit(),
            state:   AtomicU8::new(Self::UNINITIALIZED),
            _pinned: PhantomPinned,
        }
    }

    /// Like [`Self::init_with`] but uses `is_shared = false` and `sem_count = 0`.
    ///
    /// This is a common use case to have a `sem_t` that is private to a single process (i.e. not
    /// shareable between multiple) and that starts with a "resource count" of zero so that
    /// initial waiting on it blocks waiter threads until a post indicates to wake.
    ///
    /// # Errors
    /// Same as [`Self::init_with`].
    #[inline]
    pub fn init(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, bool> { self.init_with(false, 0) }

    /// Do [`sem_init()`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_init.html)
    /// on an underlying `sem_t`, and return a [`SemaphoreRef`] to it.
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
    #[allow(
        clippy::missing_inline_in_public_items,
        clippy::unwrap_in_result,
        clippy::missing_panics_doc
    )]
    pub fn init_with(
        self: Pin<&Self>,
        is_shared: bool,
        sem_count: c_uint,
    ) -> Result<SemaphoreRef<'_>, bool> {
        // Since our crate is `no_std`, `Once` or `OnceLock` are not available in only the `core`
        // lib, so we do our own once-ness with an atomic.
        match self.state.compare_exchange(Self::UNINITIALIZED, Self::PREPARING, Relaxed, Relaxed)
        {
            Ok(_) => {
                // This call is the first, so it does the initialization.

                let sem: *mut libc::sem_t = UnsafeCell::raw_get(MaybeUninit::as_ptr(&self.inner));

                // SAFETY: The arguments are valid.
                let r = unsafe {
                    libc::sem_init(
                        sem,
                        if is_shared {
                            Self::MULTI_PROCESS_SHARED
                        } else {
                            Self::SINGLE_PROCESS_PRIVATE
                        },
                        sem_count,
                    )
                };
                if r == 0 {
                    // Do `Release` to ensure that the memory writes that `sem_init()` did will be
                    // properly visible to other threads that do `Self::sem_ref`.
                    self.state.store(Self::READY, Release);
                    #[allow(clippy::expect_used)]
                    Ok(self.sem_ref().expect("the `Semaphore` is ready"))
                } else {
                    Err(false)
                }
            },
            Err(_) => Err(true),
        }
    }

    /// Get a [`SemaphoreRef`] to `self`, so that semaphore operations can be done on `self`.
    ///
    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    ///
    /// # Errors
    /// If `self` was not previously initialized.
    #[allow(clippy::missing_inline_in_public_items)]
    pub fn sem_ref(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, ()> {
        // Do `Acquire` to ensure that the memory writes that `sem_init()` did (in `Self::init`)
        // from another thread will be properly visible in our thread.
        if Self::READY == self.state.load(Acquire) {
            fn project_inner(it: &Semaphore) -> &UnsafeCell<libc::sem_t> {
                let sem = &it.inner;
                // SAFETY: `sem` is ready, so it was initialized correctly and successfully.
                unsafe { MaybeUninit::assume_init_ref(sem) }
            }
            // SAFETY: The `.inner` field is pinned when `self` is.
            let sem = unsafe { Pin::map_unchecked(self, project_inner) };
            Ok(SemaphoreRef(sem))
        } else {
            Err(())
        }
    }

    /// Like [`Self::try_init_with`] but uses `is_shared = false` and `sem_count = 0`, similar to
    /// [`Self::init`].
    #[must_use]
    #[inline]
    pub fn try_init(self: Pin<&Self>, limit: u64) -> Option<SemaphoreRef<'_>> {
        self.try_init_with(limit, false, 0)
    }

    /// Try to initialize `self`, repeatedly if necessary, if not already initialized, and return
    /// a reference to it.
    ///
    /// Will spin-loop waiting until it's initialized, up to the given `limit` of retries.
    #[must_use]
    #[inline]
    pub fn try_init_with(
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
    #[must_use]
    #[inline]
    pub fn display(self: Pin<&Self>) -> impl Display + '_ {
        struct Wrap<'l>(Pin<&'l Semaphore>);

        impl Display for Wrap<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                match self.0.sem_ref() {
                    Ok(sem) => Display::fmt(&sem, f),
                    Err(()) => write!(f, "<Semaphore>"),
                }
            }
        }

        Wrap(self)
    }
}


impl Default for Semaphore {
    #[inline]
    fn default() -> Self { Self::new() }
}


impl Drop for Semaphore {
    #[inline]
    fn drop(&mut self) {
        fn pinned_drop(this: Pin<&mut Semaphore>) {
            if let Ok(sem) = this.into_ref().sem_ref() {
                // `self` was `sem_init`ed, so it should be `sem_destroy`ed.  Because a value can
                // only be dropped if there are no borrows of or into it, this guarantees that
                // there are no `SemaphoreRef`s to `self`, and so this guarantees that there are
                // no waiters blocked on `self`, and so this guarantees that the `sem_destroy()`
                // will not fail.
                sem.destroy();
            }
        }
        // SAFETY: Okay because we know this value is never used again after being dropped.
        pinned_drop(unsafe { Pin::new_unchecked(self) });
    }
}
