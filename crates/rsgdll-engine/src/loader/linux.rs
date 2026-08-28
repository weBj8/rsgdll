use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;

use super::LibraryError;

const RTLD_NOW: c_int = 2;
const RTLD_NOLOAD: c_int = 4;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

pub(super) struct LoadedModule(NonNull<c_void>);

impl LoadedModule {
    pub(super) fn open(library: &CStr) -> Result<Self, LibraryError> {
        let path = loaded_library_path(library)?;
        // SAFETY: RTLD_NOLOAD prevents running initializers; the path names an
        // already-mapped trusted Source library and is NUL-terminated.
        NonNull::new(unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_NOLOAD) })
            .map(Self)
            .ok_or_else(|| LibraryError::new("load Source library", loader_error()))
    }

    pub(super) fn symbol(&self, symbol: &CStr) -> Result<NonNull<c_void>, LibraryError> {
        // SAFETY: POSIX specifies a null dlerror call clears prior state.
        unsafe { dlerror() };
        // SAFETY: this handle is live and the symbol name is NUL-terminated.
        NonNull::new(unsafe { dlsym(self.0.as_ptr(), symbol.as_ptr()) })
            .ok_or_else(|| LibraryError::new("resolve Source symbol", loader_error()))
    }
}

impl Drop for LoadedModule {
    fn drop(&mut self) {
        // SAFETY: this handle came from dlopen and is closed exactly once.
        unsafe { dlclose(self.0.as_ptr()) };
    }
}

fn loaded_library_path(library: &CStr) -> Result<CString, LibraryError> {
    let maps = std::fs::read_to_string("/proc/self/maps")
        .map_err(|error| LibraryError::new("inspect loaded Source libraries", error.to_string()))?;
    let library_name = library.to_string_lossy();
    let path = maps
        .lines()
        .filter_map(|line| line.split_ascii_whitespace().last())
        .find(|path| {
            Path::new(path)
                .file_name()
                .is_some_and(|name| name == library_name.as_ref())
        })
        .ok_or_else(|| {
            LibraryError::new(
                "locate loaded Source library",
                format!("{library_name} is not mapped into this process"),
            )
        })?;
    CString::new(path).map_err(|_| {
        LibraryError::new(
            "locate loaded Source library",
            "mapped library path contains a NUL byte".to_owned(),
        )
    })
}

fn loader_error() -> String {
    // SAFETY: dlerror returns either null or a NUL-terminated thread-local
    // message valid until the next dynamic-loader call on this thread.
    let message = unsafe { dlerror() };
    if message.is_null() {
        "unknown dynamic loader error".to_owned()
    } else {
        // SAFETY: non-null dlerror results are NUL-terminated strings.
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}
