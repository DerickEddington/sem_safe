# `sem_safe`

A Rust-ified, but direct, interface to [POSIX Semaphores](
https://pubs.opengroup.org/onlinepubs/9699919799/xrat/V4_xsh_chap02.html#tag_22_02_08_03)
that enforces safe [usage](
https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/semaphore.h.html)
of them.

# Example

```rust
use sem_safe::unnamed::Semaphore;
# use std::{pin::Pin, thread};

static SEMAPHORE: Semaphore = Semaphore::new();

fn main() {
    let sem = Pin::static_ref(&SEMAPHORE);
    let sem = sem.init().unwrap();
    thread::spawn(move || sem.wait().unwrap());
    sem.post().unwrap();
}
```

# Motivation

POSIX Semaphores, in particular the [`sem_post`](
https://pubs.opengroup.org/onlinepubs/9699919799/functions/sem_post.html)
function, are especially useful for an async-signal handler to wake a blocked thread, because
`sem_post()` is async-signal-safe (in contrast to many thread-waking APIs, such as channels, that
don't guarantee this).  Signal handlers still need to be very careful that everything else they do
is all async-signal-safe (such as only using atomic types to exfiltrate signal information to
other threads) but `sem_post` provides the critical ability to wake another thread (e.g. to
further handle the exfiltrated signal info in a normal context without the extreme restrictions of
async-signal safety).  (One of the very-few alternatives to `sem_post` is the "self-pipe" trick
where `write()` to a pipe is done from a signal handler, and where blocking `read()` from the
other end of the pipe is done from the other thread, but that is somewhat messier (due to needing
to setup the pipes, close-on-exec, non-blocking writes, etc).)

Signal-handling is not the only use-case.  This crate provides an analogue of the C API that can
be used for various other semaphore use-cases.  Currently, only the "unnamed" semaphores' API is
supported, for both the shared-between-multiple-processes mode or the
private-to-only-a-single-process mode.  The rest of the API for "timed-wait" and for "named"
semaphores could be implemented in the future.

# Design

The challenges with using POSIX Semaphores safely according to the Rust ways, and what this crate
provides solutions to, are:

- To share a semaphore between multiple threads, the type must be `Sync`, which requires "interior
  mutability".  This crate implements its own abstraction over `UnsafeCell<libc::sem_t>` to
  achieve this, and this also enables values of this type to be global `static` items (not `mut`)
  which can be convenient, or values of this type can be shorter-lived locals and lifetime-safety
  is enforced.

- The values of the `sem_t` type must start as uninitialized and then be initialized by calling
  `sem_init()`, before applying any of the other operations to a `sem_t`.  This crate has separate
  owned `Semaphore` and borrowed `SemaphoreRef` types to enforce that the operations can only be
  done to safe references to initialized values and that the references can only be gotten after
  pinning and initializing owned values.

- Deinitialization (`sem_destroy()`) is only done when dropping an owned `Semaphore` and only if
  it was initialized.  Dropping is prevented when there are any `SemaphoreRef`s extant, which
  prevents destroying a semaphore when there still are potential use-sites.

- It's not clear if moving a `sem_t` value is permitted after it's been initialized with
  `sem_init()`.  This crate uses `Pin`ning to enforce that the values can't be moved after having
  been initialized.

- The `sem_init()` must only be done once to a `sem_t`.  This crate uses atomics directly (because
  this crate is `no_std`) to enforce this, even if there are additional calls and perhaps from
  multiple threads concurrently.
