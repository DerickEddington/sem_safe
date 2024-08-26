use crate::UnwrapOS as _;
use sem_safe::unnamed::Semaphore;


include!("non_named.rs");


#[cfg(any())] // You could comment-out, to activate, to see that non-pinned can't work.
#[test]
fn needs_pin() {
    fn static_assert_semaphore_trait<'l>(_: &impl non_named::Semaphore<'l>) {}
    fn static_assert_unpin(_: &impl Unpin) {}

    let sem = Semaphore::default();
    static_assert_semaphore_trait(&sem);
    static_assert_unpin(&sem);
    sem.init();
    sem.sem_ref();
}


#[test]
fn drop_and_reinit() {
    let mut sem = pin!(Semaphore::uninit());
    sem.set(Semaphore::uninit()); // Drop when uninitialized.
    {
        let is_shared = cfg!(not(target_os = "openbsd"));
        let sem_ref = sem.as_ref().init_with(is_shared, 1).unwrap_os();
        assert_eq!(sem_ref.get_value(), 1);
    }
    sem.set(Semaphore::uninit()); // Drop when initialized.
    {
        let sem_ref = sem.as_ref().init_with(false, 2).unwrap_os();
        assert_eq!(sem_ref.get_value(), 2);
    }
    sem.set(Semaphore::uninit()); // Reassign to uninitialized.
    assert!(sem.as_ref().sem_ref().is_err());
}
