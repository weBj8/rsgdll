use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;

use super::LibraryError;

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetModuleHandleA"]
    fn get_module_handle(name: *const c_char) -> *mut c_void;
    #[link_name = "GetProcAddress"]
    fn get_proc_address(library: *mut c_void, name: *const c_char) -> *mut c_void;
}

pub(super) struct LoadedModule(NonNull<c_void>);

impl LoadedModule {
    pub(super) fn open(library: &CStr) -> Result<Self, LibraryError> {
        // SAFETY: the name is NUL-terminated; GetModuleHandleA borrows only an
        // already-loaded module and does not alter its lifetime.
        NonNull::new(unsafe { get_module_handle(library.as_ptr()) })
            .map(Self)
            .ok_or_else(|| LibraryError::new("load Source library", last_os_error()))
    }

    pub(super) fn symbol(&self, symbol: &CStr) -> Result<NonNull<c_void>, LibraryError> {
        // SAFETY: this borrowed module handle is live and the symbol name is
        // NUL-terminated.
        NonNull::new(unsafe { get_proc_address(self.0.as_ptr(), symbol.as_ptr()) })
            .ok_or_else(|| LibraryError::new("resolve Source symbol", last_os_error()))
    }
}

fn last_os_error() -> String {
    std::io::Error::last_os_error().to_string()
}
