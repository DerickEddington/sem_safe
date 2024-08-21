//! Anonymous "named" semaphores.

use crate::{named, InitOnce, SemaphoreRef};
use core::{cell::UnsafeCell,
           ffi::c_uint,
           fmt::{self, Display, Formatter}};


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


/// The same method names and signatures as [`unnamed::Semaphore`].
///
/// Except, these take `&self` instead of `self: Pin<&Self>`, because this type doesn't need to be
/// `Pin`ned (unlike `unnamed::Semaphore`).  This difference in the `self` type is transparent to
/// users' code that, depending on conditional compilation (e.g. by using
/// `plaster::non_named::Semaphore`), uses either this type or `unnamed::Semaphore`.  That
/// works because `Pin<&Self>` auto-derefs to `&Self`.
// TODO: verify the above
impl Semaphore {
    /// Create an uninitialized `sem_t *`.
    ///
    /// The only operations that can be done with a new instance are to [initialize](Self::init)
    /// it or drop it.
    #[inline]
    pub const fn uninit() -> Self {
        Self { inner: UnsafeCell::new(None), init_once: InitOnce::new() }
    }

    /// Like [`Self::init_with`] but uses `sem_count = 0`.
    ///
    /// This is a common use-case to have a semaphore that starts with a "resource count" of zero
    /// so that initial waiting on it blocks waiter threads until a post indicates to wake.
    ///
    /// # Errors
    /// Same as [`Self::init_with`].
    #[inline]
    pub fn init(&self) -> Result<SemaphoreRef<'_>, bool> { self.init_with(false, 0) }

    /// Do [`named::Semaphore::anonymous_with()`] to initialize `self`, and return a
    /// [`SemaphoreRef`] to it.
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
    ///
    /// # Panics
    /// If `is_shared == true`.  Anonymous semaphores cannot be shared between multiple processes
    /// (except by `fork()`), because they don't have a name for other processes to open them by.
    /// This parameter exists only to have the same function signature as
    /// [`unnamed::Semaphore::init_with`](non_named::Semaphore::init_with), but uses of
    /// it must always be `false`.
    #[allow(
        clippy::missing_inline_in_public_items,
        clippy::unwrap_in_result,
        clippy::panic_in_result_fn
    )]
    pub fn init_with(
        &self,
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

    /// Get a [`SemaphoreRef`] to `self`, so that semaphore operations can be done on `self`.
    ///
    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    ///
    /// # Errors
    /// If `self` was not previously initialized.
    #[allow(
        clippy::missing_inline_in_public_items,
        clippy::unwrap_in_result,
        clippy::missing_panics_doc
    )]
    pub fn sem_ref(&self) -> Result<SemaphoreRef<'_>, ()> {
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

    /// Return a value that displays `self`.
    ///
    /// Shows the current count value only if the semaphore has been initialized.
    ///
    /// This exists only to have the same method as [`unnamed::Semaphore::display`].
    #[must_use]
    #[inline]
    pub fn display(&self) -> impl Display + '_ {
        struct Wrap<'l>(&'l Semaphore);
        impl Display for Wrap<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { Display::fmt(&self.0, f) }
        }
        Wrap(self)
    }

    // TODO: The same exact API as unnamed::Semaphore, except `&self` instead of `self:
    // Pin<&Self>`
}


impl Default for Semaphore {
    #[inline]
    fn default() -> Self { Self::uninit() }
}


/// Shows the current count value only if the semaphore has been initialized.
impl Display for Semaphore {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // TODO: the impl in `unnamed` should be factored-out for reuse.
        todo!()
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
