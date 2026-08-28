//! Raw Source engine ABI definitions.
//!
//! See `docs/engine-abi-reference.md` for the pinned upstream definition.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(any(
    all(target_os = "linux", target_env = "gnu", target_arch = "x86"),
    all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"),
)))]
compile_error!("rsgdll-engine-sys supports only GNU Linux and MSVC Windows on x86 or x86_64");

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

/// Engine library candidates used by GMod client and dedicated-server builds.
#[cfg(all(target_os = "linux", target_arch = "x86"))]
pub const ENGINE_LIBRARIES: &[&CStr] = &[c"engine_srv.so", c"engine.so"];
/// Engine library candidates used by GMod client and dedicated-server builds.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const ENGINE_LIBRARIES: &[&CStr] = &[c"engine.so", c"engine_client.so", c"engine_srv.so"];
/// Engine library candidates used by GMod client and dedicated-server builds.
#[cfg(target_os = "windows")]
pub const ENGINE_LIBRARIES: &[&CStr] = &[c"engine.dll", c"engine_srv.dll"];

/// Tier-zero library candidates used by GMod client and dedicated-server builds.
#[cfg(all(target_os = "linux", target_arch = "x86"))]
pub const TIER0_LIBRARIES: &[&CStr] = &[c"libtier0_srv.so", c"libtier0.so"];
/// Tier-zero library candidates used by GMod client and dedicated-server builds.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const TIER0_LIBRARIES: &[&CStr] = &[c"libtier0.so", c"libtier0_client.so", c"libtier0_srv.so"];
/// Tier-zero library candidates used by GMod client and dedicated-server builds.
#[cfg(target_os = "windows")]
pub const TIER0_LIBRARIES: &[&CStr] = &[c"tier0.dll", c"tier0_s.dll"];

/// Pinned dedicated-server engine interface version.
pub const VENGINE_SERVER_VERSION: &CStr = c"VEngineServer021";

/// Exported tier0 logging-listener registration symbol.
pub const REGISTER_LOGGING_LISTENER_SYMBOL: &CStr = c"LoggingSystem_RegisterLoggingListener";

/// Exported tier0 logging-listener removal symbol.
pub const UNREGISTER_LOGGING_LISTENER_SYMBOL: &CStr = c"LoggingSystem_UnregisterLoggingListener";

#[cfg(all(target_os = "windows", target_arch = "x86"))]
macro_rules! engine_method_fn {
    (($($argument:ty),* $(,)?) -> $return_type:ty) => {
        unsafe extern "thiscall" fn($($argument),*) -> $return_type
    };
}

#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
macro_rules! engine_method_fn {
    (($($argument:ty),* $(,)?) -> $return_type:ty) => {
        unsafe extern "C" fn($($argument),*) -> $return_type
    };
}

type ChangeLevelFn = engine_method_fn!((
    *mut RawEngineServer,
    *const c_char,
    *const c_char,
) -> ());
type IsMapValidFn = engine_method_fn!((*mut RawEngineServer, *const c_char) -> c_int);
type IsDedicatedServerFn = engine_method_fn!((*mut RawEngineServer) -> bool);
type ServerCommandFn = engine_method_fn!((*mut RawEngineServer, *const c_char) -> ());
type LoggingListenerLogFn = engine_method_fn!((
    *mut RawLoggingListener,
    *const RawLoggingContext,
    *const c_char,
) -> ());

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
    pub change_level: ChangeLevelFn,
    /// `IVEngineServer::IsMapValid`.
    pub is_map_valid: IsMapValidFn,
    /// `IVEngineServer::IsDedicatedServer`.
    pub is_dedicated_server: IsDedicatedServerFn,
    /// Slots 3 through 35, pinned but intentionally not exposed.
    pub before_server_command: [UnusedEngineMethod; 33],
    /// `IVEngineServer::ServerCommand`.
    pub server_command: ServerCommandFn,
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
    pub log: LoggingListenerLogFn,
}

pub type RegisterLoggingListenerFn = unsafe extern "C" fn(listener: *mut RawLoggingListener);
pub type UnregisterLoggingListenerFn = unsafe extern "C" fn(listener: *mut RawLoggingListener);

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn linux_x86_64_supports_single_player_libraries() {
        assert!(ENGINE_LIBRARIES.contains(&c"engine_client.so"));
        assert!(TIER0_LIBRARIES.contains(&c"libtier0_client.so"));
    }
}
