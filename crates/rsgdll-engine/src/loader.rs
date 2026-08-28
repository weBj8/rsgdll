//! Checked Source engine interface wrappers.

#![deny(unsafe_op_in_unsafe_fn)]

use std::error::Error;
use std::ffi::{CStr, CString, c_void};
use std::fmt;
use std::ptr::NonNull;

use rsgdll_engine_sys::{
    CREATE_INTERFACE_SYMBOL, CreateInterfaceFn, ENGINE_LIBRARIES, IFACE_FAILED, IFACE_OK,
};

#[cfg(target_os = "linux")]
#[path = "loader/linux.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "loader/windows.rs"]
mod platform;

use platform::LoadedModule;

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
    _module: LoadedModule,
    factory: InterfaceFactory,
}

impl EngineLibrary {
    /// Attaches to the dedicated-server engine library.
    ///
    /// # Safety
    ///
    /// The process must be a compatible Source dedicated server with its
    /// trusted engine binary already loaded.
    pub(crate) unsafe fn open_engine() -> Result<Self, LibraryError> {
        let mut last_error = None;
        for library in ENGINE_LIBRARIES {
            // SAFETY: the caller accepts the loading and ABI requirements above.
            match unsafe { Self::open(library) } {
                Ok(engine) => return Ok(engine),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            LibraryError::new(
                "load Source library",
                "no engine library names configured".into(),
            )
        }))
    }

    /// Attaches to a loaded Source library and resolves `CreateInterface`.
    ///
    /// # Safety
    ///
    /// `library` must identify an already-loaded trusted library compatible
    /// with the pinned Source factory ABI.
    unsafe fn open(library: &CStr) -> Result<Self, LibraryError> {
        let module = LoadedModule::open(library)?;
        let symbol = module.symbol(CREATE_INTERFACE_SYMBOL)?;
        // SAFETY: the platform loader returns the named C export, and the
        // caller guarantees the Source factory ABI.
        let factory =
            unsafe { std::mem::transmute::<*mut c_void, CreateInterfaceFn>(symbol.as_ptr()) };

        Ok(Self {
            _module: module,
            // SAFETY: the module remains live until after the final factory use.
            factory: unsafe { InterfaceFactory::from_raw(factory) },
        })
    }

    pub(crate) fn query(&self, name: &CStr) -> Result<NonNull<c_void>, InterfaceError> {
        self.factory.query(name)
    }
}

/// Resolves one C-exported symbol already loaded into the current process.
///
/// # Safety
///
/// Caller must convert the result only to the symbol's exact ABI type.
pub(crate) unsafe fn process_symbol(
    libraries: &[&CStr],
    symbol: &CStr,
) -> Result<NonNull<c_void>, LibraryError> {
    let mut last_error = None;
    for library in libraries {
        match LoadedModule::open(library) {
            Ok(module) => return module.symbol(symbol),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        LibraryError::new("load Source library", "no library names configured".into())
    }))
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LibraryError {
    operation: &'static str,
    detail: String,
}

impl LibraryError {
    pub(super) fn new(operation: &'static str, detail: String) -> Self {
        Self { operation, detail }
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.detail)
    }
}

impl Error for LibraryError {}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_int};

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
