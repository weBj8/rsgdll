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

/// Linux Source tier-zero runtime library.
pub const TIER0_LIBRARY: &CStr = c"libtier0.so";

/// Pinned dedicated-server engine interface version.
pub const VENGINE_SERVER_VERSION: &CStr = c"VEngineServer021";

/// Exported tier0 logging-listener registration symbol.
pub const REGISTER_LOGGING_LISTENER_SYMBOL: &CStr = c"LoggingSystem_RegisterLoggingListener";

/// Exported tier0 logging-listener removal symbol.
pub const UNREGISTER_LOGGING_LISTENER_SYMBOL: &CStr = c"LoggingSystem_UnregisterLoggingListener";

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
    /// Slots 3 through 35, pinned but intentionally not exposed.
    pub before_server_command: [UnusedEngineMethod; 33],
    /// `IVEngineServer::ServerCommand`.
    pub server_command: unsafe extern "C" fn(this: *mut RawEngineServer, command: *const c_char),
}

/// An unexposed `IVEngineServer021` vtable slot.
pub type UnusedEngineMethod = unsafe extern "C" fn();

/// Source logging severity from the pinned x86-64 `tier0/logging.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LoggingSeverity {
    Message = 0,
    Warning = 1,
    Assert = 2,
    Error = 3,
}

/// `LCF_DO_NOT_ECHO` from the pinned x86-64 `tier0/logging.h`.
pub const LOGGING_DO_NOT_ECHO: c_int = 0x0000_0002;

/// Prefix consumed from Source's `LoggingContext_t`.
#[repr(C)]
pub struct RawLoggingContext {
    pub channel_id: c_int,
    pub flags: c_int,
    pub severity: c_int,
    pub color: [u8; 4],
}

/// Raw `ILoggingListener` object.
#[repr(C)]
pub struct RawLoggingListener {
    pub vtable: *const RawLoggingListenerVTable,
}

/// Pinned single-method `ILoggingListener` vtable.
#[repr(C)]
pub struct RawLoggingListenerVTable {
    pub log: unsafe extern "C" fn(
        this: *mut RawLoggingListener,
        context: *const RawLoggingContext,
        message: *const c_char,
    ),
}

pub type RegisterLoggingListenerFn = unsafe extern "C" fn(listener: *mut RawLoggingListener);
pub type UnregisterLoggingListenerFn = unsafe extern "C" fn(listener: *mut RawLoggingListener);
