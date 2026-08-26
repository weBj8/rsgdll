//! Checked Source engine interface wrappers.

#![deny(unsafe_op_in_unsafe_fn)]

use std::error::Error;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::path::Path;
use std::ptr::NonNull;

use rsgdll_engine_sys::{
    CREATE_INTERFACE_SYMBOL, CreateInterfaceFn, ENGINE_LIBRARY, IFACE_FAILED, IFACE_OK,
};

const RTLD_NOW: c_int = 2;
const RTLD_NOLOAD: c_int = 4;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

#[derive(Clone, Copy)]
struct InterfaceFactory(CreateInterfaceFn);

impl InterfaceFactory {
    /// Wraps a raw Source interface factory.
    ///
    /// # Safety
    ///
    /// `factory` must remain callable and implement the pinned Source
    /// `CreateInterfaceFn` ABI for every query made through this value.
    #[must_use]
    const unsafe fn from_raw(factory: CreateInterfaceFn) -> Self {
        Self(factory)
    }

    fn query(self, name: &CStr) -> Result<NonNull<c_void>, InterfaceError> {
        let mut return_code = IFACE_FAILED;
        // SAFETY: `from_raw` establishes the function ABI and lifetime. `name`
        // and `return_code` remain valid for the duration of this call.
        let pointer = unsafe { (self.0)(name.as_ptr(), &mut return_code) };
        match NonNull::new(pointer) {
            Some(pointer) if return_code == IFACE_OK => Ok(pointer),
            _ => Err(InterfaceError::NotFound {
                name: name.to_owned(),
                return_code,
            }),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InterfaceError {
    NotFound { name: CString, return_code: i32 },
}

impl fmt::Display for InterfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name, return_code } => write!(
                formatter,
                "Source interface {} was not found (factory status {return_code})",
                name.to_string_lossy()
            ),
        }
    }
}

impl Error for InterfaceError {}

pub(crate) struct EngineLibrary {
    handle: NonNull<c_void>,
    factory: InterfaceFactory,
}

impl EngineLibrary {
    /// Loads the Linux dedicated-server engine library.
    ///
    /// # Safety
    ///
    /// Loading a shared object executes its initializers. The process must be a
    /// compatible Source dedicated server with its trusted engine binary
    /// already loaded.
    pub(crate) unsafe fn open_engine() -> Result<Self, LibraryError> {
        let maps = std::fs::read_to_string("/proc/self/maps").map_err(|error| {
            LibraryError::new("inspect loaded Source libraries", error.to_string())
        })?;
        let library_name = ENGINE_LIBRARY.to_string_lossy();
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
                    "locate loaded Source engine",
                    format!("{library_name} is not mapped into this process"),
                )
            })?;
        let path = CString::new(path).map_err(|_| {
            LibraryError::new(
                "locate loaded Source engine",
                "mapped library path contains a NUL byte".to_owned(),
            )
        })?;
        // SAFETY: the caller accepts the loading and ABI requirements above.
        unsafe { Self::open(&path) }
    }

    /// Attaches to a loaded Source library and resolves `CreateInterface`.
    ///
    /// # Safety
    ///
    /// `path` must identify an already-loaded trusted library compatible with
    /// the pinned Source factory ABI.
    unsafe fn open(path: &CStr) -> Result<Self, LibraryError> {
        // SAFETY: `RTLD_NOLOAD` prevents loading a new object or running its
        // constructors; `path` is NUL-terminated and valid for the call.
        let handle = NonNull::new(unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_NOLOAD) })
            .ok_or_else(|| LibraryError::new("load Source library", loader_error()))?;

        // SAFETY: POSIX specifies a null `dlerror` call clears prior state.
        unsafe { dlerror() };
        // SAFETY: `handle` is live and the symbol name is NUL-terminated.
        let symbol =
            NonNull::new(unsafe { dlsym(handle.as_ptr(), CREATE_INTERFACE_SYMBOL.as_ptr()) });
        let Some(symbol) = symbol else {
            let error = LibraryError::new("resolve CreateInterface", loader_error());
            // SAFETY: `handle` was returned by `dlopen` and remains open.
            unsafe { dlclose(handle.as_ptr()) };
            return Err(error);
        };
        // SAFETY: POSIX permits converting a `dlsym` result to the matching
        // function-pointer type; the caller guarantees the Source ABI.
        let factory =
            unsafe { std::mem::transmute::<*mut c_void, CreateInterfaceFn>(symbol.as_ptr()) };

        Ok(Self {
            handle,
            // SAFETY: the symbol is owned by `handle`, which this value keeps
            // open until after its final factory use.
            factory: unsafe { InterfaceFactory::from_raw(factory) },
        })
    }

    pub(crate) fn query(&self, name: &CStr) -> Result<NonNull<c_void>, InterfaceError> {
        self.factory.query(name)
    }
}

impl Drop for EngineLibrary {
    fn drop(&mut self) {
        // SAFETY: this handle came from `dlopen` and is closed exactly once.
        unsafe { dlclose(self.handle.as_ptr()) };
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LibraryError {
    operation: &'static str,
    detail: String,
}

impl LibraryError {
    fn new(operation: &'static str, detail: String) -> Self {
        Self { operation, detail }
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.detail)
    }
}

impl Error for LibraryError {}

fn loader_error() -> String {
    // SAFETY: `dlerror` returns either null or a NUL-terminated thread-local
    // message valid until the next dynamic-loader call on this thread.
    let message = unsafe { dlerror() };
    if message.is_null() {
        "unknown dynamic loader error".to_owned()
    } else {
        // SAFETY: non-null `dlerror` results are NUL-terminated strings.
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static VALUE: u8 = 7;

    unsafe extern "C" fn factory(name: *const c_char, return_code: *mut c_int) -> *mut c_void {
        // SAFETY: `InterfaceFactory::query` supplies a valid C string.
        let found = unsafe { CStr::from_ptr(name) } == c"TestInterface001";
        if !return_code.is_null() {
            // SAFETY: `InterfaceFactory::query` supplies one writable integer.
            unsafe { return_code.write(if found { IFACE_OK } else { IFACE_FAILED }) };
        }
        if found {
            std::ptr::from_ref(&VALUE).cast_mut().cast()
        } else {
            std::ptr::null_mut()
        }
    }

    #[test]
    fn factory_requires_ok_status_and_non_null_pointer() {
        // SAFETY: this fake implements the pinned factory ABI.
        let factory = unsafe { InterfaceFactory::from_raw(factory) };
        assert_eq!(
            factory
                .query(c"TestInterface001")
                .expect("known interface")
                .cast::<u8>()
                .as_ptr(),
            std::ptr::from_ref(&VALUE).cast_mut()
        );
        assert_eq!(
            factory.query(c"MissingInterface001"),
            Err(InterfaceError::NotFound {
                name: c"MissingInterface001".into(),
                return_code: IFACE_FAILED,
            })
        );
    }
}
