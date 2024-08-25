// This file is `include`d by multiple modules, so that these tests are done for all those
// different types.

#[cfg(not(any(feature = "unnamed", feature = "anonymous")))]
core::compile_error!("This test group needs one of the non-named kinds.");

use crate::errno;
use core::{pin::{pin, Pin},
           sync::atomic::{AtomicI32, Ordering::Relaxed},
           time::Duration};
//
use sem_safe::non_named::{self, Semaphore as _};
//
use std::{io,
          thread::{self, sleep}};

// Note: The `Semaphore` type is imported by the module that `include`s this file.


#[test]
fn common() {
    static SEMAPHORE: Semaphore = Semaphore::uninit();

    fn main() {
        let semaphore = Pin::static_ref(&SEMAPHORE);
        semaphore.init().unwrap_os();
        let sem = semaphore.sem_ref().unwrap();
        let t = thread::spawn(move || {
            sem.wait().unwrap_os();
            sem.wait().unwrap_os();
        });
        sem.post().unwrap_os();
        sleep(Duration::from_secs(2));
        sem.post().unwrap_os();
        t.join().unwrap();

        #[cfg(not(target_os = "macos"))]
        {
            let val = sem.get_value();
            assert_eq!(val, 0);
        }
    }

    main();
}


#[test]
fn rarer() {
    fn f() {
        #[cfg_attr(
            target_os = "macos",
            allow(unused_variables, unit_bindings, clippy::let_unit_value)
        )]
        let val = {
            #[allow(clippy::default_trait_access)]
            let semaphore: Pin<&mut Semaphore> = pin!(Default::default());
            let sem = semaphore.into_ref().try_init_with(0, false, 1).unwrap();
            thread::scope(|scope| {
                scope.spawn(|| {
                    sem.post().unwrap_os();
                    sem.post().unwrap_os();
                    sem.post().unwrap_os();
                    sem.post().unwrap_os();
                });
            });
            sem.try_wait().unwrap_os(); // Count is at least 1, regardless of the racing.
            sem.wait().unwrap_os();
            sem.wait().unwrap_os();

            #[cfg(not(target_os = "macos"))]
            {
                sem.get_value()
            }
        };
        #[cfg(not(target_os = "macos"))]
        assert_eq!(val, 2);
    }
    f();
}


#[test]
fn init_only_once() {
    let semaphore = pin!(Semaphore::uninit());
    let semaphore = semaphore.into_ref();
    assert!(semaphore.sem_ref().is_err());
    semaphore.init().unwrap_os();
    assert!(semaphore.sem_ref().is_ok());
    assert_eq!(semaphore.init(), Err(true));
    assert!(semaphore.sem_ref().is_ok());
}


#[cfg(not(target_os = "netbsd"))] // NetBSD's SEM_VALUE_MAX == UINT_MAX
#[test]
fn init_failure() {
    static SEMAPHORE: Semaphore = Semaphore::uninit();
    let semaphore = Pin::static_ref(&SEMAPHORE);
    // This value exceeds `SEM_VALUE_MAX`.
    let excessive_value = core::ffi::c_uint::MAX;
    assert!(semaphore.sem_ref().is_err());
    let r = semaphore.init_with(false, excessive_value);
    assert_eq!(r, Err(false));
    assert_eq!(errno(), libc::EINVAL);
    assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::InvalidInput);
    assert!(semaphore.sem_ref().is_err());
}


// Note: Run this test with --show-output to see the formatting.
#[test]
#[allow(clippy::print_stdout, clippy::dbg_macro)]
fn fmt() {
    let semaphore = pin!(Semaphore::default());
    let semaphore = semaphore.into_ref();
    println!("Displayed uninit: {}", semaphore.display());
    dbg!(semaphore);
    {
        semaphore.init_with(false, 123).unwrap_os();
        println!("Displayed ready: {}", semaphore.display());
        dbg!(semaphore);
        dbg!(semaphore.sem_ref()).unwrap();
        println!("Displayed ref: {}", semaphore.sem_ref().unwrap());
    }
}


#[test]
fn memory_ordering() {
    static SEMAPHORE: Semaphore = Semaphore::uninit();
    static ANOTHER_OBJECT: AtomicI32 = AtomicI32::new(1);

    let semaphore = Pin::static_ref(&SEMAPHORE).init().unwrap_os();

    let t = thread::spawn(move || {
        semaphore.wait().unwrap_os();
        // `sem_wait()` synchronizes memory with `sem_post()`, and so the store done by our other
        // thread, before its `sem_post()`, will be visible to us now.
        assert_eq!(2, ANOTHER_OBJECT.load(Relaxed));
    });

    ANOTHER_OBJECT.store(2, Relaxed);
    // `sem_post()` synchronizes memory with `sem_wait()`, and so the preceding store done by us
    // will be visible to our other thread when it returns from `sem_wait()`.
    semaphore.post().unwrap_os();

    t.join().unwrap();
}


#[cfg(not(target_os = "netbsd"))] // NetBSD's SEM_VALUE_MAX == UINT_MAX
#[test]
fn try_init_failure() {
    let sem = pin!(Semaphore::uninit());
    let sem = sem.into_ref();
    // This value exceeds `SEM_VALUE_MAX`.
    let excessive_value = core::ffi::c_uint::MAX;
    let r = sem.try_init_with(u64::MAX, false, excessive_value);
    assert!(r.is_none());
}


#[test]
fn generic() {
    fn o<O: non_named::Semaphore>() {
        let pinned_mut = pin!(O::default());
        let pinned = pinned_mut.into_ref();
        s(pinned);
    }
    fn s<S: non_named::Semaphore>(sem: Pin<&S>) {
        let ref1 = sem.init().unwrap_os();
        {
            let r = ref1.try_wait();
            assert!(r.is_err());
            assert_eq!(errno(), libc::EAGAIN);
            assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        }
        ref1.post().unwrap_os();
        let ref2 = sem.try_init(0).unwrap();
        ref2.try_wait().unwrap_os();
    }

    o::<Semaphore>();

    let sem = pin!(Semaphore::uninit());
    let sem = sem.into_ref();
    s(sem);
}
