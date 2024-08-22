# `sem_safe`

An interface to [POSIX Semaphores](
https://pubs.opengroup.org/onlinepubs/9799919799/xrat/V4_xsh_chap01.html#tag_22_02_08_03)
that is Rust-ified, but direct, and `no_std`, and enforces safe [usage](
https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html)
of them.

# Example

```rust
// (The `Semaphore` type under the `plaster` module enables portability even to macOS.)
use sem_safe::plaster::non_named::Semaphore;
use std::{pin::Pin, thread, sync::atomic::{AtomicI32, Ordering::Relaxed}};

static SEMAPHORE: Semaphore = Semaphore::uninit();
static THING: AtomicI32 = AtomicI32::new(0);

fn main() {
    let sem = Pin::static_ref(&SEMAPHORE);
    let sem = sem.init().unwrap();

    thread::spawn(move || {
        THING.store(1, Relaxed);
        // It's guaranteed that this thread's preceding writes are always visible to other threads
        // as happens-before our post is visible to (and possibly wakes) other threads.
        sem.post().unwrap();
    });

    sem.wait().unwrap();
    // It's guaranteed that this thread always sees the other thread's write as happens-before
    // this thread sees the other thread's post (that woke us if we'd waited).
    assert_eq!(1, THING.load(Relaxed));
}
```

# Motivation

POSIX Semaphores, in particular the [`sem_post`](
https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_post.html)
function, are especially useful for an async-signal handler to wake a blocked thread, because
`sem_post()` is async-signal-safe (in contrast to many thread-waking APIs, such as
`Thread::unpark` or channels, that don't guarantee this).  `sem_post` provides the critical
ability to wake another thread (e.g. to further handle exfiltrated representations of the received
signals in a normal context (without the extreme restrictions of async-signal safety)), from
within an extremely-limited signal handler.

Signal-handling is not the only use-case.  POSIX Semaphores also enable various patterns of
coordinating and synchronizing multiple processes, which could be compelling.  This crate provides
an analogue of the C API that can be used for various other semaphore use-cases.  Both the
*unnamed* and the *named* semaphores APIs are supported, for both the
shared-between-multiple-processes mode or the private-to-only-a-single-process mode.  The rest of
the API for "timed-wait" could be implemented in the future.

Unlike `std::thread` parking, this crate does not require the `std` library, and this crate's
semaphores can wake multiple threads on a single semaphore, can model resource counts greater than
one, can be used between multiple processes, and this crate's `SemaphoreRef::post` guarantees
async-signal-safety.

# Design

The challenges with using POSIX Semaphores safely and in the Rust ways, and what this crate
provides solutions to, are:

- To share a semaphore between multiple threads, the type must be `Sync`, which requires "interior
  mutability".  This crate implements its own abstractions over `UnsafeCell<libc::sem_t>` or
  `*mut libc::sem_t` to achieve this, and this also enables values of these to be global `static`
  items (not `mut`) which can be convenient, or values of these can be shorter-lived locals and
  lifetime-safety is enforced.

- The values of the unnamed `sem_t` type must start as uninitialized and then be initialized by
  calling `sem_init()`, and the values of the named `sem_t *` must be initialized by calling
  `sem_open()`, before applying any of the other operations to them.  This crate has separate
  owned `Semaphore` and borrowed `SemaphoreRef` types to enforce that the operations can only be
  done to safe references to initialized values and that the references can only be gotten after
  initializing owned values, which first requires pinning for the unnamed type.  This also ensures
  thread safety.

- Deinitialization (`sem_destroy()` or (safely) `sem_close()`) is only done when dropping an owned
  `Semaphore` and only if it was initialized.  Dropping is prevented when there are any
  `SemaphoreRef`s extant, which prevents invalidating a semaphore when there still are potential
  use-sites.  This also ensures avoidance of undefined behavior.

- It's not clear if moving a `sem_t` value is permitted after it's been initialized with
  `sem_init()`.  The POSIX and OpenIndiana `man` pages say that "copies" (which would be at
  addresses different than where initialized) would be undefined, which might imply that moved
  values could also be.  This crate uses `Pin`ning to enforce that the values can't be moved once
  initialized.

- The `sem_init()` must only be done once to a `sem_t`.  Creating an anonymous semaphore must only
  do `sem_open()` once.  This crate uses atomics directly (because this crate is `no_std`) to
  enforce this, even if there are additional calls and perhaps from multiple threads concurrently.

# Portability

This crate was confirmed to build and pass its tests on (x86_64 only so far):

- BSD
  - FreeBSD 14.0
  - NetBSD 9.1
- Linux
  - Alpine 3.18 (uses musl)
  - Debian 12
  - NixOS 24.05
  - Ubuntu 23.10
- Solaris
  - OpenIndiana 2023.10

All glibc- or musl-based Linux OSs should already work.  It might already work on further POSIX
OSs.  If not, adding support for other POSIX OSs should be easy but might require making tweaks to
this crate's conditional compilation and/or linking.

### macOS Partially Unsupportable

Unfortunately, macOS (and Mac OS X) does not provide the unnamed semaphores API (in violation of
modern POSIX versions requiring it), and so it's not possible for that aspect of this crate to
work on macOS.  However, this crate's support for the named semaphores does work on macOS because
it does provide that.  This crate provides a helper to create *anonymous* "named" semaphores that
are mostly like unnamed private semaphores, and this crate provides an abstraction for use across
all OSs that uses the anonymous or unnamed semaphores depending on the OS's support, for use-cases
of non-named private semaphores that need a workaround on macOS.
