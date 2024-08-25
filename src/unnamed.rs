//! Unnamed semaphores.

use crate::{non_named::{self, InitOnce},
            SemaphoreRef};
use core::{cell::UnsafeCell,
           ffi::{c_int, c_uint},
           marker::PhantomPinned,
           mem::MaybeUninit,
           pin::Pin};


/// An "unnamed" [`sem_t`](
/// https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html)
/// that can only be used safely.
///
/// This must remain pinned for and after [`Self::init_with()`](non_named::Semaphore::init_with),
/// because it's [not clear](
/// https://pubs.opengroup.org/onlinepubs/9799919799/functions/V2_chap02.html#tag_16_09_09)
/// if moving a `sem_t` value is permitted after it's been initialized with `sem_init()`.  Using
/// this as a `static` item (not as `mut`able) is a common way to achieve that (via
/// [`Pin::static_ref`]).  Or, [`pin!`](core::pin::pin) can also work.
#[must_use]
#[derive(Debug)]
pub struct Semaphore {
    // Note: This deliberately has `MaybeUninit` as the outer type with `UnsafeCell` inside that,
    // because this better facilitates our need to have `&UnsafeCell<libc::sem_t>` for
    // `SemaphoreRef`.  This is sound [1][2][3][4].  Transposing them could work but would
    // require much uglier and more `unsafe` code for our uses.
    // [1] https://doc.rust-lang.org/core/cell/struct.UnsafeCell.html#method.raw_get
    // [2] https://doc.rust-lang.org/core/mem/union.MaybeUninit.html#method.as_ptr
    // [3] https://github.com/rust-lang/rust/issues/65216#issuecomment-539958078
    // [4] https://lore.kernel.org/stable/20231106130308.041864552@linuxfoundation.org/
    inner:     MaybeUninit<UnsafeCell<libc::sem_t>>,
    init_once: InitOnce,
    _pinned:   PhantomPinned,
}


/// SAFETY: The POSIX Semaphores API intends for `sem_t`, after initialization, to be shared
/// between threads and its operations are thread-safe (similar to atomic types).  Our API ensures
/// by construction that multiple threads can only operate on an instance after initialization.
/// Therefore we can expose this in Rust as having "thread-safe interior mutability".  The other
/// field is `InitOnce` which already is `Sync`.
unsafe impl Sync for Semaphore {}

// Note: `Send` is already automatically impl'ed.  Note: sending, or otherwise moving, a `sem_t`
// value is only possible before it's initialized with `Self::init_with()`; and once it's
// initialized it's pinned and so cannot be moved, and so cannot be sent, thereafter.


impl Semaphore {
    // These values are decided by the `sem_init` documentation.
    const SINGLE_PROCESS_PRIVATE: c_int = 0;
    const MULTI_PROCESS_SHARED: c_int = 1;

    /// Create an uninitialized `sem_t`.
    ///
    /// The only operations that can be done with a new instance are to [initialize](
    /// non_named::Semaphore::init_with) it (which first requires pinning it) or drop it.
    #[inline]
    pub const fn uninit() -> Self {
        Self {
            inner:     MaybeUninit::uninit(),
            init_once: InitOnce::new(),
            _pinned:   PhantomPinned,
        }
    }

    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    fn ready_ref(self: Pin<&Self>) -> Option<Pin<&'_ UnsafeCell<libc::sem_t>>> {
        #![allow(clippy::if_then_some_else_none)]
        if self.init_once.is_ready() {
            fn project_inner_init(it: &Semaphore) -> &UnsafeCell<libc::sem_t> {
                let sem = &it.inner;
                // SAFETY: `sem` is ready, so it was initialized correctly and successfully.
                unsafe { MaybeUninit::assume_init_ref(sem) }
            }
            // SAFETY: The `.inner` field is pinned when `self` is.
            let sem = unsafe { Pin::map_unchecked(self, project_inner_init) };
            Some(sem)
        } else {
            None
        }
    }
}


impl non_named::Sealed for Semaphore {}

impl non_named::Semaphore for Semaphore {
    /// Do [`sem_init()`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_init.html)
    /// on an underlying `sem_t`, and return a [`SemaphoreRef`] to it.
    #[inline]
    #[allow(clippy::unwrap_in_result)]
    fn init_with(
        self: Pin<&Self>,
        is_shared: bool,
        sem_count: c_uint,
    ) -> Result<SemaphoreRef<'_>, bool> {
        let r = self.init_once.call_once(|| {
            let sem: *mut libc::sem_t = UnsafeCell::raw_get(MaybeUninit::as_ptr(&self.inner));
            // SAFETY: The arguments are valid.
            let r = unsafe {
                libc::sem_init(
                    sem,
                    if is_shared {
                        Semaphore::MULTI_PROCESS_SHARED
                    } else {
                        Semaphore::SINGLE_PROCESS_PRIVATE
                    },
                    sem_count,
                )
            };
            if r == 0 { Ok(()) } else { Err(()) }
        });
        match r {
            #[allow(clippy::expect_used)]
            Some(Ok(())) => Ok(self.sem_ref().expect("the `Semaphore` is ready")),
            Some(Err(())) => Err(false),
            None => Err(true),
        }
    }

    #[inline]
    fn sem_ref(self: Pin<&Self>) -> Result<SemaphoreRef<'_>, ()> {
        self.ready_ref()
            .map(|sem| {
                // SAFETY: `Some(sem)` means that `sem` is initialized by `sem_init()`.
                unsafe { SemaphoreRef::unnamed(sem) }
            })
            .ok_or(())
    }
}


impl Default for Semaphore {
    #[inline]
    fn default() -> Self { Self::uninit() }
}


impl Drop for Semaphore {
    #[inline]
    fn drop(&mut self) {
        fn pinned_drop(this: Pin<&mut Semaphore>) {
            if let Some(sem) = this.into_ref().ready_ref() {
                // SAFETY: `sem` was `sem_init`ed, so it should be `sem_destroy`ed.  Because a
                // value can only be dropped if there are no borrows of or into it, this
                // guarantees that there are no `SemaphoreRef`s to `self`, and so this guarantees
                // that there are no waiters blocked on `sem`, and so this guarantees that the
                // `sem_destroy()` will not fail nor cause undefined behavior.
                let r = unsafe { libc::sem_destroy(sem.get()) };
                debug_assert_eq!(r, 0, "the semaphore is valid with no waiters");
            }
        }
        // SAFETY: Okay because we know this value is never used again after being dropped.
        pinned_drop(unsafe { Pin::new_unchecked(self) });
    }
}


#[cfg(doctest)]
mod compile_fail_tests {
    /// ```compile_fail
    /// use sem_safe::{unnamed::Semaphore, non_named::Semaphore as _};
    /// let sem_unpinned = Semaphore::default();
    /// sem_unpinned.init();
    /// sem_unpinned.sem_ref();
    /// ```
    fn must_pin() {}

    /// ```compile_fail
    /// use sem_safe::{unnamed::Semaphore, non_named::Semaphore as _};
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
