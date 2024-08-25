use crate::UnwrapOS as _;
use sem_safe::anonymous::Semaphore;


include!("non_named.rs");


#[test]
fn is_unpin() {
    fn static_assert_unpin(_: &impl Unpin) {}
    let sem = Semaphore::default();
    static_assert_unpin(&sem);
}


#[test]
fn methods_still_need_pin() {
    let sem = pin!(Semaphore::default());
    let sem = sem.into_ref();
    sem.init().unwrap_os();
    sem.try_init(0).unwrap();
    sem.sem_ref().unwrap();
    let _d = sem.display();
}


#[test]
fn drop() {
    let mut sem = pin!(Semaphore::uninit());
    sem.as_ref().init().unwrap_os();
    sem.set(Semaphore::uninit());
    assert!(sem.as_ref().sem_ref().is_err());
}
