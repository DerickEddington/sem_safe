#![cfg(test)] // Suppress `clippy::tests_outside_test_module` lint.
#![allow(
    clippy::unwrap_used,
    unused_results,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use core::{
    pin::{pin, Pin},
    time::Duration,
};
use sem_safe::unnamed::Semaphore;
use std::thread::{self, sleep};

#[test]
fn common() {
    static SEMAPHORE: Semaphore = Semaphore::new();

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
            let semaphore = pin!(Semaphore::new());
            let sem = semaphore.into_ref().init_with(true, 1).unwrap();
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
    let semaphore = pin!(Semaphore::new());
    let semaphore = semaphore.into_ref();
    semaphore.init().unwrap();
    assert_eq!(semaphore.init(), Err(true));
}

#[test]
fn init_failure() {
    static SEMAPHORE: Semaphore = Semaphore::new();
    let semaphore = Pin::static_ref(&SEMAPHORE);
    assert_eq!(
        semaphore.init_with(true, core::ffi::c_uint::MAX),
        Err(false)
    );
}

// Note: Run this test with --show-output to see the formatting.
#[test]
#[allow(clippy::print_stdout, clippy::dbg_macro)]
fn fmt() {
    let semaphore = pin!(Semaphore::new());
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
