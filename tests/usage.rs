#![cfg(test)] // Suppress `clippy::tests_outside_test_module` lint.
#![allow(
    clippy::unwrap_used,
    unused_results,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]
#[cfg(not(any(feature = "unnamed", feature = "anonymous")))]
core::compile_error!("This test group needs one of the non-named kinds.");

use core::{pin::{pin, Pin},
           sync::atomic::{AtomicI32, Ordering::Relaxed},
           time::Duration};
//
#[cfg(all(not(feature = "plaster"), feature = "anonymous"))]
use sem_safe::anonymous::Semaphore;
//
#[cfg(feature = "plaster")]
use sem_safe::plaster::non_named::Semaphore;
//
#[cfg(all(not(feature = "plaster"), not(feature = "anonymous")))]
use sem_safe::unnamed::Semaphore;
//
use std::thread::{self, sleep};


#[test]
fn common() {
    static SEMAPHORE: Semaphore = Semaphore::uninit();

    fn main() {
        let semaphore = Pin::static_ref(&SEMAPHORE);
        semaphore.init().unwrap();
        let sem = semaphore.sem_ref().unwrap();
        let t = thread::spawn(move || {
            sem.wait().unwrap();
            sem.wait().unwrap();
        });
        sem.post().unwrap();
        sleep(Duration::from_secs(2));
        sem.post().unwrap();
        t.join().unwrap();
        let val = sem.get_value();
        assert_eq!(val, 0);
    }

    main();
}


#[test]
fn rarer() {
    fn f() {
        let val = {
            let semaphore = pin!(Semaphore::uninit());
            let sem = semaphore.into_ref().init_with(false, 1).unwrap();
            thread::scope(|scope| {
                scope.spawn(|| {
                    sem.post().unwrap();
                    sem.post().unwrap();
                    sem.post().unwrap();
                    sem.post().unwrap();
                });
            });
            sem.try_wait().unwrap(); // Count is at least 1, regardless of the racing.
            sem.wait().unwrap();
            sem.wait().unwrap();
            sem.get_value()
        };
        assert_eq!(val, 2);
    }
    f();
}


#[test]
fn init_only_once() {
    let semaphore = pin!(Semaphore::uninit());
    let semaphore = semaphore.into_ref();
    semaphore.sem_ref().unwrap_err();
    semaphore.init().unwrap();
    semaphore.sem_ref().unwrap();
    assert_eq!(semaphore.init(), Err(true));
    semaphore.sem_ref().unwrap();
}


#[cfg(not(target_os = "netbsd"))] // NetBSD's SEM_VALUE_MAX == UINT_MAX
#[test]
fn init_failure() {
    static SEMAPHORE: Semaphore = Semaphore::uninit();
    let semaphore = Pin::static_ref(&SEMAPHORE);
    // This value exceeds `SEM_VALUE_MAX` and so will cause an `EINVAL` error.
    let excessive_value = core::ffi::c_uint::MAX;
    semaphore.sem_ref().unwrap_err();
    assert_eq!(semaphore.init_with(false, excessive_value), Err(false));
    semaphore.sem_ref().unwrap_err();
}


// Note: Run this test with --show-output to see the formatting.
#[test]
#[allow(clippy::print_stdout, clippy::dbg_macro)]
fn fmt() {
    let semaphore = pin!(Semaphore::uninit());
    let semaphore = semaphore.into_ref();
    println!("Displayed uninit: {}", semaphore.display());
    dbg!(semaphore);
    {
        semaphore.init_with(false, 123).unwrap();
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

    let semaphore = Pin::static_ref(&SEMAPHORE).init().unwrap();

    let t = thread::spawn(move || {
        semaphore.wait().unwrap();
        // `sem_wait()` synchronizes memory with `sem_post()`, and so the store done by our other
        // thread, before its `sem_post()`, will be visible to us now.
        assert_eq!(2, ANOTHER_OBJECT.load(Relaxed));
    });

    ANOTHER_OBJECT.store(2, Relaxed);
    // `sem_post()` synchronizes memory with `sem_wait()`, and so the preceding store done by us
    // will be visible to our other thread when it returns from `sem_wait()`.
    semaphore.post().unwrap();

    t.join().unwrap();
}
