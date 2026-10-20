//! Locating already-loaded engine libraries (port of garrysmod_common `ModuleLoader.cpp`, spec §5.2).

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CString;

use crate::gmod::{CreateInterfaceFn, IFACE_OK};

#[cfg(windows)]
const PATTERNS: &[&str] = &["bin/win64/{}.dll", "garrysmod/bin/{}.dll", "bin/{}.dll", "{}.dll"];

#[cfg(unix)]
const PATTERNS: &[&str] = &[
    "bin/linux64/lib{}_client.so",
    "bin/linux64/{}_client.so",
    "bin/linux64/lib{}.so",
    "bin/linux64/{}.so",
    "bin/linux32/lib{}.so",
    "bin/linux32/{}.so",
    "garrysmod/bin/{}_srv.so",
    "garrysmod/bin/{}.so",
    "bin/lib{}.so",
    "bin/{}.so",
    "lib{}.so",
    "{}.so",
];

/// Candidate file names for `module`, in lookup order.
pub fn candidates(module: &str) -> impl Iterator<Item = String> + '_ {
    PATTERNS.iter().map(move |p| p.replace("{}", module))
}

#[cfg(unix)]
fn open_loaded(path: &str) -> Option<libloading::Library> {
    use libloading::os::unix::{Library, RTLD_NOW};
    const RTLD_NOLOAD: c_int = 0x4; // glibc; never loads a new copy
    // SAFETY: NOLOAD only returns a handle to an already-initialised library.
    unsafe { Library::open(Some(path), RTLD_NOW | RTLD_NOLOAD) }.ok().map(Into::into)
}

#[cfg(windows)]
fn open_loaded(path: &str) -> Option<libloading::Library> {
    libloading::os::windows::Library::open_already_loaded(path).ok().map(Into::into)
}

/// Finds an already-loaded engine library by base name (`"lua_shared"`, `"engine"`, ...).
pub fn find_module(module: &str) -> Option<libloading::Library> {
    candidates(module).find_map(|c| open_loaded(&c))
}

/// Calls the library's `CreateInterface(name)`; `None` unless it returns a non-null `IFACE_OK`.
///
/// # Safety
/// The caller must cast the result to the matching interface layout.
pub unsafe fn create_interface(lib: &libloading::Library, name: &str) -> Option<*mut c_void> {
    let name = CString::new(name).ok()?;
    let f: CreateInterfaceFn = unsafe { *lib.get(b"CreateInterface\0").ok()? };
    let mut code: c_int = IFACE_OK;
    let p = unsafe { f(name.as_ptr() as *const c_char, &mut code) };
    (!p.is_null() && code == IFACE_OK).then_some(p)
}

/// Keeps the DLL containing `addr` mapped for the process lifetime (spec §8.1).
#[cfg(unix)]
// `addr` is only an address for dladdr lookup, never dereferenced.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn pin_self(addr: *const c_void) -> bool {
    use libloading::os::unix::{Library, RTLD_NOW};
    const RTLD_NODELETE: c_int = 0x1000;
    #[repr(C)]
    struct DlInfo {
        fname: *const c_char,
        fbase: *mut c_void,
        sname: *const c_char,
        saddr: *mut c_void,
    }
    unsafe extern "C" {
        fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
    }
    let mut info = DlInfo { fname: core::ptr::null(), fbase: core::ptr::null_mut(), sname: core::ptr::null(), saddr: core::ptr::null_mut() };
    // SAFETY: dladdr only reads loader tables; fname is a C string owned by the loader.
    let path = unsafe { (dladdr(addr, &mut info) != 0 && !info.fname.is_null()).then(|| std::ffi::CStr::from_ptr(info.fname)) };
    path.and_then(|p| p.to_str().ok())
        // SAFETY: re-opening an already loaded image with NODELETE; the handle is leaked on purpose.
        .and_then(|p| unsafe { Library::open(Some(p), RTLD_NOW | RTLD_NODELETE) }.ok())
        .map(std::mem::forget)
        .is_some()
}

#[cfg(windows)]
pub fn pin_self(addr: *const c_void) -> bool {
    const GET_MODULE_HANDLE_EX_FLAG_PIN: u32 = 0x1;
    const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, name: *const c_void, out: *mut *mut c_void) -> i32;
    }
    let mut h = core::ptr::null_mut();
    // SAFETY: FROM_ADDRESS treats `addr` as an address inside the module.
    unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, addr, &mut h) != 0 }
}
