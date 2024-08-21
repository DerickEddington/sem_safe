//! Since our crate is `no_std`, `Once` or `OnceLock` are not available in only the `core` lib, so
//! we do our own once-ness with an atomic.

use core::sync::atomic::{AtomicU8,
                         Ordering::{Acquire, Relaxed, Release}};


#[derive(Debug)]
pub(crate) struct InitOnce(AtomicU8);

impl InitOnce {
    const UNINITIALIZED: u8 = 0;
    const PREPARING: u8 = 1;
    const READY: u8 = 2;

    pub(crate) const fn new() -> Self { Self(AtomicU8::new(Self::UNINITIALIZED)) }

    pub(crate) fn call_once<T, E>(
        &self,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        match self.0.compare_exchange(Self::UNINITIALIZED, Self::PREPARING, Relaxed, Relaxed) {
            Ok(_) => {
                let r = f();
                if r.is_ok() {
                    // Do `Release` to ensure that any memory writes done by `f()` will be
                    // properly visible to other threads that might need to see them.
                    self.0.store(Self::READY, Release);
                }
                Some(r)
            },
            Err(_) => None,
        }
    }

    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    pub(crate) fn is_ready(&self) -> bool {
        // Do `Acquire` to ensure that any memory writes that the `f()` of `self.call_once()` did
        // from another thread will be properly visible in our thread.
        self.0.load(Acquire) == Self::READY
    }
}


#[cfg(test)]
mod tests {
    #![allow(clippy::unreachable, clippy::unwrap_used)]
    use super::*;
    use core::{hint, sync::atomic::AtomicI32};
    extern crate std;
    use std::thread;

    #[test]
    fn is_ready() {
        let x = InitOnce::new();
        assert!(!x.is_ready());
        let r1 = x.call_once(|| Result::<_, ()>::Ok(1 + 1));
        assert_eq!(r1, Some(Ok(2)));
        assert!(x.is_ready());
        let r2: Option<Result<(), bool>> = x.call_once(|| unreachable!());
        assert!(r2.is_none());
        assert!(x.is_ready());
        let r3: Option<Result<bool, i32>> = x.call_once(|| unreachable!());
        assert!(r3.is_none());
        assert!(x.is_ready());
    }

    #[test]
    fn failure_isnt_ready() {
        let x = InitOnce::new();
        let r1 = x.call_once(|| Result::<(), _>::Err(()));
        assert_eq!(r1, Some(Err(())));
        assert!(!x.is_ready());
        assert_eq!(x.0.load(Relaxed), InitOnce::PREPARING);
        let r2: Option<Result<(), ()>> = x.call_once(|| unreachable!());
        assert!(r2.is_none());
        assert!(!x.is_ready());
        assert_eq!(x.0.load(Relaxed), InitOnce::PREPARING);
    }

    #[test]
    fn synchronizes() {
        static THING: AtomicI32 = AtomicI32::new(0);
        static X: InitOnce = InitOnce::new();

        let t = thread::spawn(|| {
            X.call_once(|| {
                // This write to memory will be visible to the other thread.
                THING.store(1, Relaxed);
                Result::<_, ()>::Ok(()) // `Ok` return does the needed "release".
            })
            .unwrap()
            .unwrap();
        });

        while !X.is_ready() {
            thread::yield_now();
            hint::spin_loop();
        }
        // The synchronization only happens once the instance is "ready".  `.is_ready()` does the
        // needed "acquire".
        assert_eq!(THING.load(Relaxed), 1);

        // It's essential to the proper exercising of this test that this join is not done until
        // after our testing, because this would also synchronize the memory.
        t.join().unwrap(); // Just to clean-up.
    }
}
