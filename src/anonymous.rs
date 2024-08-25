//! Anonymous "named" semaphores.

use crate::{named,
            non_named::{self, InitOnce},
            SemaphoreRef};
use core::{cell::UnsafeCell,
           ffi::c_uint,
           fmt::{self, Display, Formatter},
           pin::Pin};


/// An [anonymous](named::Semaphore::anonymous_with) [`named::Semaphore`] that is used like an
/// [`unnamed::Semaphore`](crate::unnamed::Semaphore) (and like that, can be included in
/// `static`s).
///
/// Because it's anonymous and freshly created, an instance of this type represents ownership of
/// its underlying OS semaphore that is exclusive such that nothing else can access that and no
/// other instances for that can exist.  To preserve ownership semantics, this isn't `Copy` nor
/// `Clone`.
#[must_use]
#[derive(Debug)]
pub struct Semaphore {
    inner:     UnsafeCell<Option<named::Semaphore>>,
    init_once: InitOnce,
}


/// SAFETY: The `inner` `UnsafeCell` is only accessed soundly by our internal implementation, and
/// indirect access to `inner` by multiple threads is correctly synchronized and has proper
/// happens-before relations.  The other contained types are `named::Semaphore` and `InitOnce`
/// which already are `Sync`.
unsafe impl Sync for Semaphore {}

// Note: `Send` is already automatically impl'ed.


impl Semaphore {
    /// Create an uninitialized `sem_t *`.
    ///
    /// The only operations that can be done with a new instance are to [initialize](
    /// non_named::Semaphore::init_with) it or drop it.
    #[inline]
    pub const fn uninit() -> Self {
        Self { inner: UnsafeCell::new(None), init_once: InitOnce::new() }
    }
}


impl non_named::Sealed for Semaphore {}

impl non_named::Semaphore for Semaphore {
    /// Do [`named::Semaphore::anonymous_with()`] to initialize `self`, and return a
    /// [`SemaphoreRef`] to it.
    ///
    /// # Panics
    /// If `is_shared == true`.  Anonymous semaphores cannot be shared between multiple processes
    /// (except by `fork()`), because they don't have a name for other processes to open them by.
    /// This parameter exists only to have the same function signature as
    /// [`unnamed::Semaphore::init_with`](non_named::Semaphore::init_with), but uses of
    /// it must always be `false`.
    #[inline]
    #[allow(clippy::unwrap_in_result, clippy::panic_in_result_fn)]
    fn init_with(
        self: Pin<&Self>,
        is_shared: bool,
        sem_count: c_uint,
    ) -> Result<SemaphoreRef<'_>, bool> {
        assert!(!is_shared, "`is_shared` should always be `false`");

        let r = self.init_once.call_once(|| {
            named::Semaphore::anonymous_with(sem_count).map(|sem| {
                // SAFETY: Within this scope there are no other references to `self.inner`'s
                // contents, so ours is effectively unique.  This is ensured by our type's
                // encapsulation that ensures that only one thread can execute this once, and that
                // other threads cannot access `self.inner`'s contents until after this init is
                // completed.  Our `.init_once.call_once()` ensures that our write here has a
                // proper happens-before relation to all other accesses (via the memory ordering
                // between that and `.init_once.is_ready()`).
                let inner_exclusive = unsafe { &mut *self.inner.get() };
                *inner_exclusive = Some(sem);
            })
        });
        match r {
            #[allow(clippy::expect_used)]
            Some(Ok(())) => Ok(self.sem_ref().expect("the `Semaphore` is ready")),
            Some(Err(())) => Err(false),
            None => Err(true),
        }
    }

    #[inline]
    #[allow(clippy::unwrap_in_result)]
    fn sem_ref(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, ()> {
        if self.init_once.is_ready() {
            // SAFETY: After it's been initialized, nothing expects to have exclusive access to
            // `self.inner`'s contents (except our `Drop` impl, but that's sound), so we can have
            // multiple shared accesses concurrently and can release (return) our immutable
            // reference to safe code.  Our `.init_once.is_ready()` ensures that reads of
            // `self.inner`'s contents have a proper happens-after relation to the write to it
            // done by `self.init_with()` (via the memory ordering between those).
            let inner_shared = unsafe { &*self.inner.get() };
            #[allow(clippy::expect_used)]
            Ok(inner_shared
               .as_ref()
               .map(named::Semaphore::sem_ref)
               // Even though panicking is not async-signal-safe, this `expect()` is fine because
               // it's truly impossible.
               .expect("always `Some` once initialized"))
        } else {
            Err(())
        }
    }
}


impl Default for Semaphore {
    #[inline]
    fn default() -> Self { Self::uninit() }
}


/// Shows the current count value only if the semaphore has been initialized.
impl Display for Semaphore {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        use crate::non_named::Semaphore as _;
        Pin::new(self).display().fmt(f)
    }
}


impl Drop for Semaphore {
    #[inline]
    fn drop(&mut self) {
        if let Some(sem) = self.inner.get_mut().take() {
            // SAFETY: The underlying `sem_t *` was `sem_open`ed, so it can be `sem_close`ed.
            // There are no other instances for the underlying OS semaphore, because our
            // encapsulated type guarantees this.  Because a value can only be dropped if there
            // are no borrows of or into it, this guarantees that there are no `SemaphoreRef`s to
            // `self`, and so this guarantees that there are no waiters blocked on the underlying
            // semaphore.
            let r = unsafe { sem.close() };
            debug_assert!(r.is_ok(), "the semaphore is valid");
        }
    }
}


#[cfg(doctest)]
mod compile_fail_tests {
    /// ```compile_fail
    /// use sem_safe::{anonymous::Semaphore, non_named::Semaphore as _};
    /// use core::pin::pin;
    /// let sem_ref = {
    ///     let sem = pin!(Semaphore::uninit());
    ///     let sem = sem.into_ref();
    ///     let sem_ref = sem.sem_ref().unwrap();
    ///     sem_ref
    /// };
    /// sem_ref.post().unwrap();
    /// ```
    fn lifetime_enforced() {}
}
