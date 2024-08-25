use crate::UnwrapOS as _;
use sem_safe::plaster::non_named::Semaphore;


include!("non_named.rs");


#[test]
fn drop() {
    let mut sem = pin!(Some(Semaphore::uninit()));
    sem.as_ref().as_pin_ref().unwrap().init().unwrap_os();
    sem.set(None);
}
