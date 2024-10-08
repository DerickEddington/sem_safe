#[cfg(not(target_os = "macos"))]
use core::ffi::c_int;
#[cfg(feature = "named")]
use core::marker::PhantomData;
#[cfg(all(feature = "unnamed", not(target_os = "macos")))]
use core::pin::Pin;
use core::{cell::UnsafeCell,
           fmt::{self, Debug, Display, Formatter},
           ptr};


/// Like a `sem_t *` as a borrow of a semaphore that is known to be initialized and so valid to do
/// operations on.
#[derive(Copy, Clone)]
pub struct SemaphoreRef<'l>(Kind<'l>);

#[derive(Copy, Clone)]
enum Kind<'l> {
    #[cfg(all(feature = "unnamed", not(target_os = "macos")))]
    Unnamed(Pin<&'l UnsafeCell<libc::sem_t>>),
    #[cfg(feature = "named")]
    Named(
        *mut libc::sem_t,
        // Needed for when this variant is the only variant (otherwise, there'd be an error about
        // "`'l` is never used").
        PhantomData<&'l UnsafeCell<libc::sem_t>>,
    ),
}


/// SAFETY: The POSIX Semaphores API intends for `sem_t *`, after the pointed-to instance is
/// initialized, to be shared between threads and its operations are thread-safe (similar to
/// atomic types).  Our API ensures by construction that multiple threads can only operate on a
/// `sem_t *` after initialization.  Therefore we can expose this in Rust as having "thread-safe
/// interior mutability".
unsafe impl Sync for SemaphoreRef<'_> {}
/// SAFETY: Ditto.
unsafe impl Send for SemaphoreRef<'_> {}


macro_rules! mem_sync_of_wait_et_al {
        () => {
        "\n\nThis synchronizes memory with respect to other threads on all successful calls.  \
        That is a primary use-case so that other threads' memory writes to other objects, \
        sequenced before [`Self::post()`], will be visible to the current thread \
        after returning from this.  If this returns an error, it is unspecified whether the \
        invocation causes memory to be synchronized.  (See: [POSIX's requirements](\
        https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html#tag_04_15_02).)"
        }
    }

impl<'l> SemaphoreRef<'l> {
    #![cfg_attr(
        not(all(feature = "unnamed", not(target_os = "macos"))),
        allow(single_use_lifetimes)
    )]
    #[cfg(all(feature = "unnamed", not(target_os = "macos")))]
    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    ///
    /// # Safety
    /// The given `sem_t` must already have been initialized by `sem_init()`.  (Otherwise,
    /// becoming a `Self` would allow the operations on it when uninitialized but that would be
    /// UB.)
    pub(crate) unsafe fn unnamed(inited: Pin<&'l UnsafeCell<libc::sem_t>>) -> Self {
        Self(Kind::Unnamed(inited))
    }

    #[cfg(feature = "named")]
    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    ///
    /// # Safety
    /// The given `sem_t *` must already have been initialized by `sem_open()`.  (Otherwise,
    /// becoming a `Self` would allow the operations on it when uninitialized but that would be
    /// UB.)  The semaphore referenced by `opened` must remain open for the duration of the
    /// returned `Self`.
    pub(crate) unsafe fn named(opened: *mut libc::sem_t) -> Self {
        Self(Kind::Named(opened, PhantomData))
    }

    fn raw(&self) -> *mut libc::sem_t {
        match self.0 {
            #[cfg(all(feature = "unnamed", not(target_os = "macos")))]
            Kind::Unnamed(cell) => cell.get(),
            #[cfg(feature = "named")]
            Kind::Named(ptr, _) => ptr,
        }
    }

    /// Like [`sem_post`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_post.html),
    /// and async-signal-safe like that.
    ///
    /// It is safe for this to be called from a signal handler.  That is a primary use-case for
    /// POSIX Semaphores versus other better synchronization APIs (which shouldn't be used in
    /// signal handlers).
    ///
    /// This synchronizes memory with respect to other threads on all successful calls.  That is a
    /// primary use-case so that memory writes to other objects, sequenced before a call to this,
    /// will be visible to other threads after returning from [`Self::wait()`] (et al).  If this
    /// returns an error, it is unspecified whether the invocation causes memory to be
    /// synchronized.  (See: [POSIX's requirements](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html#tag_04_15_02).)
    ///
    /// # Errors
    /// If `sem_post()` does.  `errno` is set to indicate the error.  Its `EINVAL` case should be
    /// impossible.
    #[inline]
    pub fn post(&self) -> Result<(), ()> {
        // SAFETY: The argument is valid, because the `Semaphore` was initialized.
        let r = unsafe { libc::sem_post(self.raw()) };
        if r == 0 {
            Ok(())
        } else {
            Err(()) // Most likely: EOVERFLOW (max value for a semaphore would be exceeded).
        }
    }

    /// Like [`sem_wait`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_wait.html).
    ///
    /// Might block the calling thread.
    #[doc = mem_sync_of_wait_et_al!()]
    ///
    /// # Errors
    /// If `sem_wait()` does.  `errno` is set to indicate the error.  Its `EINVAL` case should be
    /// impossible.
    #[inline]
    pub fn wait(&self) -> Result<(), ()> {
        // SAFETY: The argument is valid, because the `Semaphore` was initialized.
        let r = unsafe { libc::sem_wait(self.raw()) };
        if r == 0 {
            Ok(())
        } else {
            Err(()) // Most likely: EINTR (a signal interrupted this function).
        }
    }

    /// Like [`sem_trywait`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_trywait.html).
    ///
    /// Won't block the calling thread.
    #[doc = mem_sync_of_wait_et_al!()]
    ///
    /// # Errors
    /// If `sem_trywait()` does.  `errno` is set to indicate the error.  Its `EINVAL` case should
    /// be impossible.
    #[inline]
    pub fn try_wait(&self) -> Result<(), ()> {
        // SAFETY: The argument is valid, because the `Semaphore` was initialized.
        let r = unsafe { libc::sem_trywait(self.raw()) };
        if r == 0 {
            Ok(())
        } else {
            Err(()) // Most likely: EAGAIN (would block), or EINTR
        }
    }

    // TODO: `Self::timedwait` that uses `sem_timedwait`.
    // TODO?: `Self::clockwait` that uses the new `sem_clockwait`?
    // TODO: The doc-comments for those will also need `#[doc = mem_sync_of_wait_et_al!()]`.

    #[cfg(not(target_os = "macos"))]
    /// Like [`sem_getvalue`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_getvalue.html).
    #[must_use]
    #[inline]
    pub fn get_value(&self) -> c_int {
        let mut sval = c_int::MIN;
        // SAFETY: The arguments are valid, because the `Semaphore` was initialized.
        let r = unsafe { libc::sem_getvalue(self.raw(), &mut sval) };
        debug_assert_eq!(r, 0, "the semaphore should be valid");
        sval
    }
}


/// Compare by `sem_t *` pointer equality.
impl PartialEq for SemaphoreRef<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool { ptr::eq(self.raw(), other.raw()) }
}

impl Eq for SemaphoreRef<'_> {}


/// Shows the `sem_t *` pointer.
impl Debug for SemaphoreRef<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SemaphoreRef").field(&self.raw()).finish()
    }
}

#[cfg(not(target_os = "macos"))]
/// Human-readable representation that shows the semaphore's current count value.
impl Display for SemaphoreRef<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "<Semaphore value:{}>", self.get_value())
    }
}

#[cfg(target_os = "macos")]
/// Human-readable representation.  Mac doesn't support getting the current count value.
impl Display for SemaphoreRef<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { write!(f, "<Semaphore inited>") }
}
