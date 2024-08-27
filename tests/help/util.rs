#![allow(dead_code)]

use core::ffi::c_int;
use std::{ffi::CString, io, process};


pub(crate) fn name(sub: &str) -> CString { name_with(process::id(), sub) }

pub(crate) fn name_with(pid: u32, sub: &str) -> CString {
    if cfg!(target_os = "macos") {
        const PSEMNAMLEN: usize = 31; // The max limit on Mac.
        let s = format!("/SS-{sub}-{pid}");
        #[allow(clippy::indexing_slicing, clippy::string_slice)]
        let s = &s[.. (PSEMNAMLEN - 1).min(s.len())]; // `- 1` leaves room for the nul.
        let n = CString::new(s).unwrap();
        assert!(n.as_bytes_with_nul().len() <= PSEMNAMLEN);
        n
    } else {
        CString::new(format!("/testing-sem_safe-{pid}-{sub}")).unwrap()
    }
}


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
