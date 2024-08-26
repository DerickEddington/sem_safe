#![allow(clippy::semicolon_inside_block)]

use cfg_if::cfg_if;
use sem_safe::SemaphoreRef;

// Note: Normally, when you want "non-named", you should just use the "plaster" feature (i.e. not
// have `cfg`s like this here).  This `cfg_if` is only to enable this example to run without that
// feature, for testing.
cfg_if! { if #[cfg(target_os = "openbsd")] {
    use sem_safe::anonymous as non_named;
} else if #[cfg(feature = "plaster")] {
    use sem_safe::plaster::non_named;
} else if #[cfg(all(feature = "unnamed", not(target_os = "macos")))] {
    use sem_safe::unnamed as non_named;
} else if #[cfg(feature = "anonymous")] {
    use sem_safe::anonymous as non_named;
} }
#[cfg(feature = "named")]
use sem_safe::named;

#[path = "../../tests/help/util.rs"]
mod util;
use util::UnwrapOS as _;


pub(crate) fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let args = &args.iter().map(String::as_ref).collect::<Vec<_>>()[..];
    match args {
        // No arguments means: parent-process mode.
        [exec_filename] => primary(exec_filename),
        // Arguments means: child-process mode.
        [_, primary_pid, which @ ("one" | "two")] => {
            let primary_pid = primary_pid.parse().expect("argument must be valid");
            other(primary_pid, *which == "one");
        },
        _ => panic!("command-line arguments must be valid"),
    }
}


fn primary(exec_filename: &str) {
    use std::{os::unix::process::CommandExt as _,
              process::{self, Child, Command}};

    fn say(msg: &str) {
        println!("primary: {msg}");
    }

    let sem_f = nonnamed_inter_proc_sem();

    // Start the child processes that use the same semaphores.
    let others: [Child; 2] = ["one", "two"].map(|child_id| {
        let mut other = Command::new(exec_filename);
        other.arg(process::id().to_string()).arg(child_id);
        // SAFETY: `.post()` is async-signal-safe, and the rest of the requirements of `.pre_exec`
        // are upheld.
        unsafe { other.pre_exec(move || sem_f.post().map_errno()) };
        other.spawn().unwrap()
    });

    // Wait for the forked children to post before their `exec()`s.
    sem_f.wait().unwrap_os();
    sem_f.wait().unwrap_os();
    say("others forked, are now exec'ing");

    #[cfg(feature = "named")]
    {
        // Open the same semaphores as the other process.
        let [me, o1, o2] = NamedSem::triple(process::id());

        // Tell the other processes that we've opened the semaphores.
        o1.post();
        o2.post();
        // Wait for the others to indicate that they've opened the semaphores.
        me.wait();
        me.wait();
        say("others opened semaphores");

        // Remove the others' semaphore names (not ours), but only after they both opened them
        // all.  It'll still work.
        o2.remove_name();
        o1.remove_name();

        // Tell the other processes to proceed.
        o1.post();
        o2.post();
        // Wait for the others to indicate that they've coordinated with each other.
        me.wait();
        me.wait();
        say("others coordinated");

        // Tell the other processes to finish.
        o2.post();
        o1.post();
        // Wait for the other processes to acknowledge.
        me.wait();
        me.wait();
        say("others finishing");

        [me, o1, o2].map(|NamedSem { sem, .. }| {
            // SAFETY: There are no other instances of these semaphores that could be used,
            // because the other processes are terminating or already have and won't use either
            // again.
            unsafe { sem.close() }.unwrap_os();
        });
    }

    // Clean-up our child processes.
    let exit_statuses = others.map(|mut child| child.wait().unwrap().success());
    assert_eq!(exit_statuses, [true, true]);
}


cfg_if! { if #[cfg(feature = "named")] {

    fn other(primary_pid: u32, is_alpha: bool) {
        let say = |msg| println!("other{}: {msg}", if is_alpha { 1 } else { 2 });

        // Open the same semaphores as the primary and other processes.
        let [p, o1, o2] = NamedSem::triple(primary_pid);
        let (me, o) = if is_alpha { (o1, o2) } else { (o2, o1) };

        // Wait for the primary process to indicate that it opened the semaphores.
        me.wait();
        say("primary opened semaphores");

        // Tell the primary process that we've opened the semaphores.
        p.post();
        // Wait for the primary process to tell us to proceed.
        me.wait();
        say("proceeding");

        // Coordinate with the other process, but only after its initial coordination with the
        // primary (to ensure it can't misinterpret this post).
        o.post();
        me.wait();
        say("other opened semaphores");

        if is_alpha {
            // Remove the primary's semaphore name (not ours), but only after the other has opened
            // it.  It'll still work.
            p.remove_name();
        }

        // Tell the primary process that we've coordinated with the other.
        p.post();

        // Wait for the primary process to tell us to finish.
        me.wait();
        say("finishing");
        // Tell the primary process we're terminating.
        p.post();
    }


    use std::ffi::CString;

    struct NamedSem {
        sem:  named::Semaphore,
        name: CString,
    }

    impl NamedSem {
        fn triple(primary_pid: u32) -> [NamedSem; 3] {
            let named_sem = |name| {
                let name = CString::new(
                    format!("/example-sem_safe-multi-proc-{primary_pid}-{name}")).unwrap();
                let sem = named::Semaphore::open(&name, named::OpenFlags::Create {
                    exclusive: false, // They race to create, and the first does it.
                    mode:      0o600,
                    value:     0,
                }).unwrap_os();
                NamedSem { sem, name }
            };
            ["primary", "other1", "other2"].map(named_sem)
        }
        fn sem(&self) -> SemaphoreRef<'_> { self.sem.sem_ref() }
        fn post(&self) { self.sem().post().unwrap_os(); }
        fn wait(&self) { self.sem().wait().unwrap_os(); }
        fn remove_name(&self) { named::Semaphore::unlink(&self.name).unwrap_os(); }
    }
}
else {
    fn other(_primary_pid: u32, _is_alpha: bool) {}
} }


fn nonnamed_inter_proc_sem<'l>() -> SemaphoreRef<'l> {
    use non_named::Semaphore;
    use std::pin::Pin;

    cfg_if! { if #[cfg(all(feature = "unnamed", not(any(target_os = "macos",
                                                        target_os = "openbsd"))))] {
        /// An unnamed semaphore must be in shared memory, to actually be shared across `fork()`.
        fn mmap_shared_sem<'l>() -> Pin<&'l mut Semaphore> {
            use std::{mem::{align_of, size_of, MaybeUninit}, ptr};
            use libc::{mmap, MAP_SHARED, MAP_ANONYMOUS, PROT_READ, PROT_WRITE, MAP_FAILED};

            let enough_size = size_of::<Semaphore>().checked_mul(2).unwrap();
            // SAFETY: The arguments are proper.
            let ptr = unsafe {
                mmap(ptr::null_mut(), enough_size, PROT_READ | PROT_WRITE,
                     // (Note: POSIX 2024 standardized `MAP_ANONYMOUS`.)
                     MAP_SHARED | MAP_ANONYMOUS, -1, 0)
            };
            assert_ne!(ptr, MAP_FAILED);
            let ptr: *mut u8 = {
                let ptr: *mut u8 = ptr.cast();
                let align_offset = ptr.align_offset(align_of::<Semaphore>());
                assert!(align_offset < size_of::<Semaphore>());
                // SAFETY: The offset is in-bounds, cannot overflow `isize`, and won't wrap.
                unsafe{ ptr.add(align_offset) }
            };
            let ptr: *mut MaybeUninit<Semaphore> = ptr.cast();
            // SAFETY: `ptr` is unique (nothing else can possibly have it yet), valid (`mmap`
            // allocated it with enough size), aligned (we ensured this), and its lifetime is the
            // rest of the program (almost `'static`) (because we don't unmap nor invalidate it).
            let uninit: &mut MaybeUninit<Semaphore> = unsafe { &mut *ptr };
            let sem = uninit.write(Semaphore::uninit());
            // SAFETY: The pointee is never moved nor invalidated (because nothing else has
            // access).
            unsafe { Pin::new_unchecked(sem) }
        }

        mmap_shared_sem().into_ref().init_with(true, 0).unwrap_os()
    }
    else if #[cfg(feature = "anonymous")] {
        use sem_safe::non_named::Semaphore as _;

        static SEM_A: Semaphore = Semaphore::uninit();
        // "Named" semaphores are shared without needing to arrange our own shared memory.
        Pin::static_ref(&SEM_A).init_with(0).unwrap_os()
    } }
}
