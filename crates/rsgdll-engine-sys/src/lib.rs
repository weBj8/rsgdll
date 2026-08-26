//! Raw Source engine ABI definitions.
//!
//! See `docs/engine-abi-reference.md` for the pinned upstream definition.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("rsgdll-engine-sys has only reviewed Source ABI definitions for Linux x86_64");

use std::ffi::{CStr, c_char, c_int, c_void};

/// Exported Source interface factory ABI.
pub type CreateInterfaceFn =
    unsafe extern "C" fn(name: *const c_char, return_code: *mut c_int) -> *mut c_void;

/// `CreateInterface` returned an interface.
pub const IFACE_OK: c_int = 0;

/// `CreateInterface` did not recognize the requested interface.
pub const IFACE_FAILED: c_int = 1;

/// Export name used by Source interface factories.
pub const CREATE_INTERFACE_SYMBOL: &CStr = c"CreateInterface";

/// Linux dedicated-server engine library.
pub const ENGINE_LIBRARY: &CStr = c"engine.so";

/// Pinned dedicated-server engine interface version.
pub const VENGINE_SERVER_VERSION: &CStr = c"VEngineServer021";

/// Raw `IVEngineServer` object.
#[repr(C)]
pub struct RawEngineServer {
    /// Pointer to the pinned vtable layout.
    pub vtable: *const RawEngineServerVTable,
}

/// Verified prefix of the `IVEngineServer021` vtable.
#[repr(C)]
pub struct RawEngineServerVTable {
    /// `IVEngineServer::ChangeLevel`.
    pub change_level: unsafe extern "C" fn(
        this: *mut RawEngineServer,
        level: *const c_char,
        landmark: *const c_char,
    ),
    /// `IVEngineServer::IsMapValid`.
    pub is_map_valid:
        unsafe extern "C" fn(this: *mut RawEngineServer, name: *const c_char) -> c_int,
    /// `IVEngineServer::IsDedicatedServer`.
    pub is_dedicated_server: unsafe extern "C" fn(this: *mut RawEngineServer) -> bool,
}
