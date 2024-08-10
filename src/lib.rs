#![doc = include_str!("../README.md")]
#![no_std]
#![allow(clippy::result_unit_err)]
#[cfg(not(unix))]
core::compile_error!("Only supported on POSIX.");

/// Unnamed semaphores.
pub mod unnamed {
    use core::{cell::UnsafeCell,
               ffi::{c_int, c_uint},
               fmt::{self, Debug, Display, Formatter},
               hint,
               marker::PhantomPinned,
               mem::MaybeUninit,
               pin::Pin,
               ptr,
               sync::atomic::{AtomicU8,
                              Ordering::{Acquire, Relaxed, Release}}};

    /// An "unnamed" [`sem_t`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html)
    /// that can only be used safely.
    ///
    /// This must remain pinned for and after [`Self::init_with()`], because it's not clear if
    /// moving a `sem_t` value is permitted after it's been initialized with `sem_init()`.  Using
    /// this as a `static` item (not as `mut`able) is a common way to achieve that (via
    /// [`Pin::static_ref`]).  Or, [`pin!`](core::pin::pin) can also work.
    #[must_use]
    #[derive(Debug)]
    pub struct Semaphore {
        inner: MaybeUninit<UnsafeCell<libc::sem_t>>,
        state: AtomicU8,
        _pin:  PhantomPinned,
    }

    /// SAFETY: The POSIX Semaphores API intends for `sem_t` to be shared between threads and its
    /// operations are thread-safe (similar to atomic types).  Therefore we can expose this in
    /// Rust as having "interior mutability".
    unsafe impl Sync for Semaphore {}

    // Note: `Send` isn't impl'ed, because it's not clear if moving a `sem_t` value is permitted
    // after it's been initialized with `sem_init`.

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
        /// The only operations that can be done with a new instance are to
        /// [initialize](Self::init) it (which first requires pinning it) or drop it.
        #[inline]
        pub const fn new() -> Self {
            Self {
                inner: MaybeUninit::uninit(),
                state: AtomicU8::new(Self::UNINITIALIZED),
                _pin:  PhantomPinned,
            }
        }

        /// Like [`Self::init_with`] but uses `is_shared = false` and `sem_count = 0`.
        ///
        /// This is a common use case to have a `sem_t` that is private to a single process
        /// (i.e. not shareable between multiple) and that starts with a "resource count" of zero
        /// so that initial waiting on it blocks waiter threads until a post indicates to wake.
        ///
        /// # Errors
        /// Same as [`Self::init_with`].
        #[inline]
        pub fn init(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, bool> {
            self.init_with(false, 0)
        }

        /// Do [`sem_init()`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_init.html)
        /// on an underlying `sem_t`, and return a [`SemaphoreRef`] to it.
        ///
        /// Usually this should only be called once.  But this guards against multiple calls on
        /// the same instance (perhaps by multiple threads), to ensure the initialization is only
        /// done once.
        ///
        /// # Errors
        /// Returns `Err(true)` if the initialization was already successfully done, or is being
        /// done, by another call (perhaps by another thread).  Returns `Err(false)` if the call
        /// tried to do the initialization but there was an error with that, in which case `errno`
        /// is set to indicate the error.
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
            // Since our crate is `no_std`, `Once` or `OnceLock` are not available in only the
            // `core` lib, so we do our own once-ness with an atomic.
            match self.state.compare_exchange(
                Self::UNINITIALIZED,
                Self::PREPARING,
                Relaxed,
                Relaxed,
            ) {
                Ok(_) => {
                    // This call is the first, so it does the initialization.

                    let sem: *mut libc::sem_t =
                        UnsafeCell::raw_get(MaybeUninit::as_ptr(&self.inner));

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
                        // Do `Release` to ensure that the memory writes that `sem_init()` did
                        // will be properly visible to other threads that do `Self::sem_ref`.
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
        /// This function is async-signal-safe, and so it's safe for this to be called from a
        /// signal handler.
        ///
        /// # Errors
        /// If `self` was not previously initialized.
        #[allow(clippy::missing_inline_in_public_items)]
        pub fn sem_ref(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, ()> {
            // Do `Acquire` to ensure that the memory writes that `sem_init()` did (in
            // `Self::init`) from another thread will be properly visible in our thread.
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

        /// Like [`Self::try_init_with`] but uses `is_shared = false` and `sem_count = 0`, similar
        /// to [`Self::init`].
        #[must_use]
        #[inline]
        pub fn try_init(self: Pin<&Self>, limit: u64) -> Option<SemaphoreRef<'_>> {
            self.try_init_with(limit, false, 0)
        }

        /// Try to initialize `self`, repeatedly if necessary, if not already initialized, and
        /// return a reference to it.
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
                    // It was already initialized or another thread was in the middle of
                    // initializing it.
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
                    // `self` was `sem_init`ed, so it should be `sem_destroy`ed.  Because a value
                    // can only be dropped if there are no borrows of or into it, this guarantees
                    // that there are no `SemaphoreRef`s to `self`, and so this guarantees that
                    // there are no waiters blocked on `self`, and so this guarantees that the
                    // `sem_destroy()` will not fail.
                    sem.destroy();
                }
            }
            // SAFETY: Okay because we know this value is never used again after being dropped.
            pinned_drop(unsafe { Pin::new_unchecked(self) });
        }
    }

    /// Like a `sem_t *` to a `sem_t` that is known to be initialized and so valid to do
    /// operations on.
    #[derive(Copy, Clone)]
    pub struct SemaphoreRef<'l>(Pin<&'l UnsafeCell<libc::sem_t>>);

    /// SAFETY: The POSIX Semaphores API intends for `sem_t *` to be shared between threads and
    /// its operations are thread-safe (similar to atomic types).  Therefore we can expose
    /// this in Rust as having "interior mutability".
    unsafe impl Sync for SemaphoreRef<'_> {}
    /// SAFETY: Ditto.
    unsafe impl Send for SemaphoreRef<'_> {}

    impl SemaphoreRef<'_> {
        /// Like [`sem_post`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_post.html),
        /// and async-signal-safe like that.
        ///
        /// It is safe for this to be called from a signal handler.  That is a primary use-case
        /// for POSIX Semaphores versus other better synchronization APIs (which shouldn't be used
        /// in signal handlers).
        ///
        /// # Errors
        /// If `sem_post()` does.  `errno` is set to indicate the error.  Its `EINVAL` case should
        /// be impossible.
        #[inline]
        pub fn post(&self) -> Result<(), ()> {
            // SAFETY: The argument is valid, because the `Semaphore` was initialized.
            let r = unsafe { libc::sem_post(self.0.get()) };
            if r == 0 {
                Ok(())
            } else {
                Err(()) // Most likely: EOVERFLOW (max value for a `sem_t` would be exceeded).
            }
        }

        /// Like [`sem_wait`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_wait.html).
        ///
        /// Might block the calling thread.
        ///
        /// # Errors
        /// If `sem_wait()` does.  `errno` is set to indicate the error.  Its `EINVAL` case should
        /// be impossible.
        #[inline]
        pub fn wait(&self) -> Result<(), ()> {
            // SAFETY: The argument is valid, because the `Semaphore` was initialized.
            let r = unsafe { libc::sem_wait(self.0.get()) };
            if r == 0 {
                Ok(())
            } else {
                Err(()) // Most likely: EINTR (a signal interrupted this function).
            }
        }

        /// Like [`sem_trywait`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_trywait.html).
        ///
        /// Might block the calling thread.
        ///
        /// # Errors
        /// If `sem_trywait()` does.  `errno` is set to indicate the error.  Its `EINVAL` case
        /// should be impossible.
        #[inline]
        pub fn try_wait(&self) -> Result<(), ()> {
            // SAFETY: The argument is valid, because the `Semaphore` was initialized.
            let r = unsafe { libc::sem_trywait(self.0.get()) };
            if r == 0 {
                Ok(())
            } else {
                Err(()) // Most likely: EAGAIN (would block), or EINTR
            }
        }

        // TODO: `Self::timedwait` that uses `sem_timedwait`.
        // TODO?: `Self::clockwait` that uses the new `sem_clockwait`?

        /// Like [`sem_getvalue`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_getvalue.html).
        #[must_use]
        #[inline]
        pub fn get_value(&self) -> c_int {
            let mut sval = c_int::MIN;
            // SAFETY: The arguments are valid, because the `Semaphore` was initialized.
            let r = unsafe { libc::sem_getvalue(self.0.get(), &mut sval) };
            debug_assert_eq!(r, 0, "the `sem_t` should be valid");
            sval
        }

        /// Like [`sem_destroy`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_destroy.html).
        /// Not public.  Only used when dropping `Semaphore`.
        fn destroy(&self) {
            // SAFETY: The argument is valid, because the `Semaphore` was initialized.
            let r = unsafe { libc::sem_destroy(self.0.get()) };
            debug_assert_eq!(r, 0, "the `sem_t` should be valid with no waiters");
        }
    }

    /// Compare by `sem_t *` pointer equality.
    impl PartialEq for SemaphoreRef<'_> {
        #[inline]
        fn eq(&self, other: &Self) -> bool { ptr::eq(self.0.get(), other.0.get()) }
    }
    impl Eq for SemaphoreRef<'_> {}

    /// Shows the `sem_t *` pointer.
    impl Debug for SemaphoreRef<'_> {
        #[inline]
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            f.debug_tuple("SemaphoreRef").field(&self.0.get()).finish()
        }
    }

    /// Human-readable representation that shows the semaphore's current count value.
    impl Display for SemaphoreRef<'_> {
        #[inline]
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "<Semaphore value:{}>", self.get_value())
        }
    }
}
