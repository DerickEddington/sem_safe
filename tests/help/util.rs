#![allow(dead_code)]

use core::ffi::c_int;
use std::io;


pub(crate) trait UnwrapOS: Sized {
    type T;
    fn map_errno(self) -> Result<Self::T, io::Error>;
    #[track_caller]
    fn unwrap_os(self) -> Self::T { self.map_errno().unwrap() }
}

impl<T> UnwrapOS for Result<T, ()> {
    type T = T;
    fn map_errno(self) -> Result<T, io::Error> { self.map_err(|()| io::Error::last_os_error()) }
}

impl<T> UnwrapOS for Result<T, bool> {
    type T = T;
    fn map_errno(self) -> Result<Self::T, io::Error> { self.map_err(|b| assert!(!b)).map_errno() }
}


pub(crate) fn errno() -> c_int { errno::errno().0 }
