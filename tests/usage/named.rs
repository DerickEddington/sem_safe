use crate::{errno, name, UnwrapOS as _};
use sem_safe::named::{OpenFlags, Semaphore};
use std::io;


#[test]
fn basic() {
    let name = &name("basic");

    let s1 = Semaphore::open(name, OpenFlags::Create {
        exclusive: true,
        mode:      0o600,
        value:     2,
    })
    .unwrap_os();

    let s2 = Semaphore::open(name, OpenFlags::Create {
        exclusive: false,
        mode:      0o000, // ignored
        value:     0,     // ignored
    })
    .unwrap_os();

    let s3 = Semaphore::open(name, OpenFlags::AccessOnly).unwrap_os();

    s2.sem_ref().wait().unwrap_os();
    s3.sem_ref().wait().unwrap_os();

    s3.sem_ref().post().unwrap_os();
    s2.sem_ref().post().unwrap_os();
    s1.sem_ref().post().unwrap_os();
    s1.sem_ref().wait().unwrap_os();
    s1.sem_ref().wait().unwrap_os();
    s1.sem_ref().wait().unwrap_os();

    [&s1, &s2, &s3].map(|sem| {
        let r = sem.sem_ref().try_wait();
        assert!(r.is_err());
        assert_eq!(errno(), libc::EAGAIN);
        assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    });

    Semaphore::unlink(name).unwrap_os();

    let r = Semaphore::unlink(name);
    assert!(r.is_err());
    assert_eq!(errno(), libc::ENOENT);
    assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::NotFound);

    drop([s2, s3]);
    // SAFETY: There are no other instances now (because we just dropped the others). There are no
    // threads blocked on it now (because we're done and nothing else should've opened our unique
    // name).
    unsafe { s1.close() }.unwrap_os();
    // We don't close `s2` & `s3` because the behavior of that with multiple opens of the same is
    // inconsistent across OSs!  If, with whatever OS, our single close above didn't actually
    // close the others, those are still open and they'd still be usable, and they'll be closed
    // when the test process exits.  But if they were all closed by our single close, which is
    // what some OSs (OpenIndiana, at least) do, then it would be undefined behavior to still use
    // them (which is why this test can't do some test of doing so).
}


#[test]
fn already_exists() {
    let name = &name("already_exists");
    let create_exclusive = || {
        Semaphore::open(name, OpenFlags::Create {
            exclusive: true,
            mode:      0o600,
            value:     0,
        })
    };
    let remove = || Semaphore::unlink(name).unwrap_os();

    drop(create_exclusive().unwrap_os());

    let r1 = create_exclusive();
    assert!(r1.is_err());
    assert_eq!(errno(), libc::EEXIST);
    assert_eq!(r1.map_errno().unwrap_err().kind(), io::ErrorKind::AlreadyExists);

    remove();
    assert!(create_exclusive().is_ok());

    remove();
    // They'll be closed automatically upon exit.
}


#[cfg_attr(any(target_os = "freebsd", // This has SEM_VALUE_MAX=INT_MAX but doesn't enforce that.
               target_os = "netbsd", target_os = "openbsd"), // These have SEM_VALUE_MAX=UINT_MAX
           ignore)]
#[test]
fn excessive_value() {
    let r = Semaphore::open(&name("excessive_value"), OpenFlags::Create {
        exclusive: true,
        mode:      0o600,
        // This value exceeds `SEM_VALUE_MAX`.
        value:     core::ffi::c_uint::MAX,
    });
    assert!(r.is_err());
    assert_eq!(errno(), libc::EINVAL);
    assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::InvalidInput);
}


#[test]
fn missing() {
    let r = Semaphore::open(&name("missing"), OpenFlags::AccessOnly);
    assert!(r.is_err());
    assert_eq!(errno(), libc::ENOENT);
    assert_eq!(r.map_errno().unwrap_err().kind(), io::ErrorKind::NotFound);
}


// Note: Run this test with --show-output to see the formatting.
#[test]
#[allow(clippy::print_stdout, clippy::dbg_macro)]
fn fmt() {
    let name = &name("fmt");
    let semaphore = Semaphore::open(name, OpenFlags::Create {
        exclusive: true,
        mode:      0o600,
        value:     42,
    })
    .unwrap_os();

    println!("Displayed: {semaphore}");
    dbg!(&semaphore);
    dbg!(semaphore.sem_ref());
    println!("Displayed ref: {}", semaphore.sem_ref());

    Semaphore::unlink(name).unwrap_os();
}


#[cfg(feature = "anonymous")]
#[test]
fn anonymous() {
    use std::ffi::CStr;

    // This is the fallback name used if `getrandom()` fails.
    const INIT_UNIQUE: &CStr =
        if let Ok(cstr) = CStr::from_bytes_with_nul(b"/yv_dzpRXevTMrIb_QpkSpg\0") {
            cstr // (We want to support Rust 1.75 but `c"..."` literals require 1.77.)
        } else {
            panic!() // (Because `.unwrap()` isn't `const`.)
        };
    // With this open, we're testing that the same name won't be used by `anonymous()` because
    // `getrandom()` succeeds and so a random name is used instead, and so `anonymous()` won't
    // fail because its name isn't already open.
    let _dos_attack = Semaphore::open(INIT_UNIQUE, OpenFlags::Create {
        exclusive: true,
        mode:      0o666,
        value:     666,
    })
    .unwrap_os();

    // This will succeed because a random name will be used instead of `INIT_UNIQUE`.
    let anon = Semaphore::anonymous().unwrap_os();
    anon.sem_ref().post().unwrap_os();
    anon.sem_ref().wait().unwrap_os();

    Semaphore::unlink(INIT_UNIQUE).unwrap_os();
}
