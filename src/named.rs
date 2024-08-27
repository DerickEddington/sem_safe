//! Named semaphores.

use crate::SemaphoreRef;
use core::{ffi::{c_uint, CStr},
           fmt::{self, Display, Formatter}};

#[cfg(not(any(target_os = "illumos", target_os = "solaris")))]
const SEM_FAILED: *mut libc::sem_t = libc::SEM_FAILED;
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
/// The `libc` crate is missing this for these OSs.
#[allow(clippy::as_conversions)]
const SEM_FAILED: *mut libc::sem_t = -1_isize as *mut libc::sem_t;


/// A "named" [semaphore](
/// https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html)
/// that can only be used safely.
///
/// An instance of this type might, depending on how you open it, represent ownership of the
/// underlying OS semaphore.  For that case, to be compatible with ownership semantics, this isn't
/// `Copy` nor `Clone`.  This ambiguity is inherent to the "named" semaphores in general, unless
/// you have some additional encapsulation.  (This issue doesn't apply to `anonymous::Semaphore`
/// which always represents ownership.)
///
/// `Drop` isn't implemented, and so the semaphore isn't automatically `sem_close()`d when an
/// instance is dropped.  This is because closing when dropping cannot be done safely in general.
/// If you need to close, you must use [`Self::close`] and ensure you uphold its safety
/// requirements.  (This issue doesn't apply to `anonymous::Semaphore` which does close on drop
/// safely.)
#[must_use]
#[derive(Debug)]
pub struct Semaphore {
    ptr: *mut libc::sem_t,
}


/// SAFETY: The POSIX Semaphores API intends for `sem_t *`, after the pointed-to instance is
/// initialized, to be shared between threads and its operations are thread-safe (similar to
/// atomic types).  Our API ensures by construction that multiple threads can only operate on a
/// `sem_t *` after initialization.  Therefore we can expose this in Rust as having "thread-safe
/// interior mutability".
unsafe impl Sync for Semaphore {}
/// SAFETY: Ditto.
unsafe impl Send for Semaphore {}


/// Arguments to [`Semaphore::open`], modeling the POSIX flags `O_CREAT` & `O_EXCL`.
///
/// This ensures that undefined behavior (which the raw flags might otherwise have) cannot occur.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OpenFlags {
    /// The semaphore is merely accessed, not created.
    AccessOnly,
    /// Create a semaphore if it does not already exist.
    Create {
        /// If `true`, cause failure if a semaphore named by the `name` argument already exists.
        /// If `false`, merely access the semaphore if it already exists.
        exclusive: bool,
        /// The permission bits of the semaphore are set to this, except those set in the file
        /// mode creation mask of the process.  If bits other than file permission bits are
        /// specified, the effect is unspecified (which might be implementation-defined).
        mode:      libc::mode_t,
        /// The semaphore is created with this initial value.  Valid initial values for
        /// semaphores are less than or equal to `SEM_VALUE_MAX`.
        value:     c_uint,
    },
}


impl Semaphore {
    /// Do [`sem_open`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_open.html)
    /// to create a named semaphore.
    ///
    /// Note that some of the behaviors of `sem_open`, and so of this function, are
    /// implementation-defined.  This enables callers to leverage such but also requires them to
    /// either avoid it or deal with it.
    ///
    /// Note that on Mac even though its `man` page says opening the same name multiple times will
    /// return "the same descriptor", it actually returns distinct FDs and this allows multiple
    /// matching closes (like some other OSs that don't use FDs).  It's only the same "descriptor"
    /// in the sense of being the same semaphore.
    ///
    /// Note that on Solaris (at least on OpenIndiana) multiple opens should not have multiple
    /// matching closes, because a first close affects all additional references to the same
    /// semaphore.  This is inconsistent with other OSs but allowed by POSIX.
    ///
    /// # Errors
    /// If `sem_open()` does.  `errno` is set to indicate the error.  Its `EINTR` case is
    /// impossible because this function retries on that.
    #[inline]
    pub fn open(name: &CStr, open_flags: OpenFlags) -> Result<Self, ()> {
        let name_as = name.as_ptr();
        loop {
            let ptr = match open_flags {
                OpenFlags::AccessOnly => {
                    let oflag = 0;
                    // SAFETY: The arguments are valid.
                    unsafe { libc::sem_open(name_as, oflag) }
                },
                OpenFlags::Create { exclusive, mode, value } => {
                    let oflag = libc::O_CREAT | if exclusive { libc::O_EXCL } else { 0 };
                    let mode = c_uint::from(mode);
                    // SAFETY: The arguments are valid.
                    unsafe { libc::sem_open(name_as, oflag, mode, value) }
                },
            };
            if ptr == SEM_FAILED {
                let errno = errno::errno().0;
                if errno == libc::EINTR {
                    continue;
                }
                break Err(());
            }
            break Ok(Self { ptr });
        }
    }

    /// Get a [`SemaphoreRef`] to `self`, so that semaphore operations can be done on `self`.
    ///
    /// This function is async-signal-safe, and so it's safe for this to be called from a signal
    /// handler.
    #[must_use]
    #[inline]
    pub fn sem_ref(&self) -> SemaphoreRef<'_> {
        // SAFETY: By construction, our field is always initialized by `sem_open()`.  The
        // underlying semaphore remains open because it cannot be closed by [`Self::close`] while
        // borrowed by the returned `SemaphoreRef` (because `Self::close` would take `self` by
        // value but that's prevented by the borrow).
        unsafe { SemaphoreRef::named(self.ptr) }
    }

    /// Do [`sem_unlink()`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_unlink.html).
    ///
    /// Make the semaphore named by `name` no longer have a name but still be usable by processes
    /// that already have it open.  Even if the same name is opened again after we unlink it, that
    /// won't be the same semaphore.
    ///
    /// # Errors
    /// If `sem_unlink()` does.  `errno` is set to indicate the error.
    #[inline]
    pub fn unlink(name: &CStr) -> Result<(), ()> {
        let name = name.as_ptr();
        // SAFETY: The argument is valid.
        let r = unsafe { libc::sem_unlink(name) };
        if r == 0 { Ok(()) } else { Err(()) }
    }

    /// Do [`sem_close()`](
    /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_close.html).
    ///
    /// Note that POSIX says: "The effect of subsequent use (after being closed) of the semaphore
    /// by this process is undefined".  This is especially relevant when the same semaphore has
    /// been opened multiple times, i.e. if you have multiple `Self` instances that represent the
    /// same semaphore - some OSs support multiple closes, that match the amount of opens, before
    /// the semaphore is invalidated; but other OSs (e.g. Solaris, or at least OpenIndiana) do not
    /// support this and the first close will invalidate all the other instances and subsequent
    /// use of them is UB (and did cause a seg-fault in my test).
    ///
    /// Note that POSIX says: "If any threads in the calling process are currently blocked on the
    /// semaphore (when closed), the behavior is undefined".
    ///
    /// # Safety
    /// - Either: ensure there are no other instances for the underlying OS semaphore when this is
    ///   called (to prevent possible subsequent use); or ensure that your code can only run on an
    ///   OS that doesn't invalidate the semaphore until all opened instances have a matching
    ///   close.
    /// - Ensure there are not any threads blocked on the semaphore when this is called.
    ///
    /// # Errors
    /// If `sem_close()` does.  `errno` is set to indicate the error.
    #[inline]
    pub unsafe fn close(self) -> Result<(), ()> {
        let sem = self.ptr;
        // SAFETY: The argument is the proper type and was created via `sem_open()` and the caller
        // must uphold our requirements.
        let r = unsafe { libc::sem_close(sem) };
        if r == 0 { Ok(()) } else { Err(()) }
    }

    #[cfg(feature = "anonymous")]
    /// Like [`Self::anonymous_with`] but uses `sem_count = 0`.
    ///
    /// This is a common use-case to have a semaphore that starts with a "resource count" of zero
    /// so that initial waiting on it blocks waiter threads until a post indicates to wake.
    ///
    /// # Errors
    /// Same as [`Self::anonymous_with`].
    #[inline]
    pub fn anonymous() -> Result<Self, ()> { Self::anonymous_with(0) }

    #[cfg(feature = "anonymous")]
    /// Create a "named" semaphore whose name practically cannot be used and so is private to the
    /// calling process (i.e. not shared between multiple processes, unless by `fork()`), similar
    /// to an "unnamed" semaphore that is private to a single process.
    ///
    /// This is especially useful on macOS (a.k.a. Mac OS X) where the "unnamed" semaphores are
    /// not provided (in violation of modern POSIX) and the "named" ones are the only option even
    /// when you don't want the nameable aspects (nor their hassles).
    ///
    /// # Errors
    /// Same as [`Self::open`].
    #[inline]
    #[allow(clippy::expect_used)]
    pub fn anonymous_with(sem_count: c_uint) -> Result<Self, ()> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use core::ops::Range;
        use getrandom::getrandom;

        struct UniqueName {
            name: [u8; Self::NAME_LEN],
        }

        impl UniqueName {
            // Note: This shouldn't be too large, in case an OS has a shorter limit on the names.
            const RAND_LEN: usize = 16; // 128-bit entropy.
            const INIT_UNIQUE: [u8; Self::RAND_LEN] = [
                // This was generated from my `/dev/urandom`.
                0xCA, 0xFF, 0xDD, 0xCE, 0x94, 0x57, 0x7A, 0xF4, 0xCC, 0xAC, 0x86, 0xFF, 0x42,
                0x99, 0x12, 0xA6,
            ];
            const NAME_LEN: usize = match base64::encoded_len(Self::RAND_LEN, false) {
                Some(len) => 1 + len + 1,
                #[allow(clippy::unreachable)]
                None => unreachable!(), // Note: compile-time only.
            };
            const B64_RANGE: Range<usize> = 1 .. (Self::NAME_LEN - 1);

            fn new() -> Self {
                let mut it = Self { name: [0; Self::NAME_LEN] };
                it.name[0] = b'/'; // Some OSs require a leading '/' character.
                it.name[Self::NAME_LEN - 1] = b'\0'; // It must be a valid C string.
                it
            }

            fn generate(&mut self) -> &CStr {
                // Init to something unique, just in case `getrandom()` fails, to avoid clashes
                // with any (legit) name already in use in the host.
                let mut random: [u8; Self::RAND_LEN] = Self::INIT_UNIQUE;
                // A random name prevents DoS attacks because it's unguessable.  On some OSs, this
                // will block until enough entropy has been collected by the system, instead of
                // fail.  For our small size, this should only be possible during early boot of
                // the host.
                let _ignore_err = getrandom(&mut random);
                // If getting randomness failed, the contents of `random` are probably still
                // usable as a name for the very-short timespan we need it, even though those
                // contents are indeterminate after such failure.  So try to use anyway instead of
                // returning failure too easily.

                // Encode as base64 to encode nuls and in case some OSs don't like other chars.
                // Use URL-safe base64 because some OSs don't support non-leading '/' chars.
                // When semaphore names appear in a file-system for some OSs and if that's
                // case-insensitive then the use of base64 would cause loss of effective entropy,
                // which is an additional reason to start with 128-bit so it'll still be somewhat
                // high and unguessable even if halved.  It would be very weird (and maybe
                // illegal) for a POSIX host to place the semaphore names in a case-insensitive
                // FS.
                let _b64_size = URL_SAFE_NO_PAD
                    .encode_slice(random, &mut self.name[Self::B64_RANGE])
                    .expect("output size is always enough");
                {
                    #![allow(clippy::used_underscore_binding)]
                    debug_assert_eq!(self.name[0], b'/', "path slash is preserved");
                    debug_assert_eq!(self.name[Self::NAME_LEN - 1], b'\0', "nul is preserved");
                    debug_assert_eq!(_b64_size, Self::NAME_LEN - 2, "all other bytes filled");
                }
                CStr::from_bytes_with_nul(&self.name).expect("nul byte is at end")
            }
        }

        const TRY_LIMIT: u32 = 10;

        let mut unique_name = UniqueName::new();
        let open_flags = OpenFlags::Create {
            exclusive: true,
            mode:      0o600, // u=rw,go= (rw-------)
            value:     sem_count,
        };

        for _ in 0 .. TRY_LIMIT {
            let name = unique_name.generate();
            // It's very unlikely that another semaphore with the same name will exist at the same
            // time we're trying to open it, because our names are very-high-entropy gibberish and
            // because we "unlink" our semaphores immediately.  If the same name somehow already
            // exists, this will fail, which is desired, because we use "exclusive".
            if let Ok(sem) = Semaphore::open(name, open_flags) {
                // Immediately make our new semaphore private to our process by no longer having a
                // name.  The only way something else (i.e. another process or some other
                // misbehaving part of our same process) could gain access to our new semaphore is
                // if that can do *all* of: guess our random name (extremely unlikely) (or, in the
                // very rare case of fallback, our unique name (unlikely)) or learn our random
                // name if it's listed in an FS briefly (unlikely) *and* open that name during the
                // very short timespan between our `open()` above and our `unlink()` here (very
                // unlikely) *and* have permission according to our restrictive `mode` (unlikely).
                // All that would be very unlikely.
                let r = Self::unlink(name);
                debug_assert!(r.is_ok(), "name unlink will succeed");

                return Ok(sem);
            };

            // Else: the attempted opening failed for some reason - maybe because of unusually low
            // system limits or unusually high resource usage or unusually restricted permissions,
            // or maybe because the name somehow already existed.  Keep trying with a different
            // random name.
        }

        // This is very unlikely, usually.  However, it becomes only unlikely if: (1) resources or
        // permissions in the host are unusually tight; or (2) `getrandom()` failed (e.g. because
        // the host very recently booted and the OS hadn't collected enough (P)RNG seed entropy
        // yet when that was called) and an attacker deliberately already opened a semaphore with
        // the same name as `INIT_UNIQUE`.  Your host should be configured to have enough
        // resources and reasonable permissions, but if it isn't then you have other problems
        // anyway.  Your host shouldn't be running software that might do this attack, but if it
        // is then you have other problems anyway.
        Err(())
    }
}


// `Eq` & `PartialEq` aren't impl'ed, because, with named semaphores, different `sem_t *` values
// can actually refer to the same semaphore.


impl Display for Semaphore {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { Display::fmt(&self.sem_ref(), f) }
}


#[cfg(all(doctest, any(target_os = "illumos", target_os = "solaris")))]
mod compile_fail_tests {
    // If `SEM_FAILED` is ever added to `libc`, we'll want to know so we can update our dep to
    // that and not need our own definition of it above.
    /// ```compile_fail
    /// let _missing = libc::SEM_FAILED;
    /// ```
    #[allow(non_snake_case)]
    fn SEM_FAILED_is_missing() {}
}
