use crate::UnwrapOS as _;
use cfg_if::cfg_if;
use sem_safe::SemaphoreRef;

cfg_if! { if #[cfg(feature = "plaster")] {
    use sem_safe::plaster::non_named::Semaphore;
} else if #[cfg(all(feature = "unnamed", not(target_os = "macos")))] {
    use sem_safe::unnamed::Semaphore;
} else if #[cfg(feature = "anonymous")] {
    use sem_safe::anonymous::Semaphore;
} else if #[cfg(feature = "named")] {
    use sem_safe::named::Semaphore;
} }


fn a_sem_ref<const B: bool>() -> (SemaphoreRef<'static>, Option<impl FnOnce()>) {
    cfg_if! { if #[cfg(any(feature = "plaster",
                           all(feature = "unnamed", not(target_os = "macos")),
                           feature = "anonymous"))] {
        use core::pin::Pin;
        use sem_safe::non_named::Semaphore as _;

        let sem = Pin::static_ref(if B {
            static SEM: Semaphore = Semaphore::uninit();
            &SEM
        } else {
            static SEM: Semaphore = Semaphore::uninit();
            &SEM
        });
        (sem.init().unwrap_os(), None::<fn()>)
    }
    else if #[cfg(feature = "named")] {
        use crate::name;
        use sem_safe::named::OpenFlags;
        use std::sync::OnceLock;

        let (sem, name) = if B {
            static SEM: OnceLock<Semaphore> = OnceLock::new();
            (&SEM, name("refs-1"))
        } else {
            static SEM: OnceLock<Semaphore> = OnceLock::new();
            (&SEM, name("refs-2"))
        };
        let sem = sem.get_or_init(
            || Semaphore::open(&name, OpenFlags::Create {
                exclusive: true,
                mode: 0o600,
                value: 0
            }).unwrap_os());
        (sem.sem_ref(), Some(move || Semaphore::unlink(&name).unwrap_os()))
    } }
}


#[test]
fn eq() {
    let (sr1, f1) = a_sem_ref::<true>();
    let (sr2, f2) = a_sem_ref::<false>();
    assert_eq!(sr1, sr1);
    assert_ne!(sr1, sr2);
    assert_eq!(sr2, sr2);
    assert_ne!(sr2, sr1);

    if let Some(unlink_name) = f1 {
        unlink_name();
    }
    if let Some(unlink_name) = f2 {
        unlink_name();
    }
}
