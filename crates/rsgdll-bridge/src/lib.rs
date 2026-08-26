//! Minimal C++ Lua error firewall.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_int};

use rsgdll_platform::__private::{LuaCFunction, RawLuaState};

/// Capacity of the stack-owned error buffer supplied by the C++ trampoline.
pub const ERROR_BUFFER_CAPACITY: u32 = 4096;

/// POD result returned after all Rust dispatcher frames have unwound normally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct DispatchResult {
    pub status: i32,
    pub return_count: i32,
    pub error_length: u32,
}

impl DispatchResult {
    #[must_use]
    pub const fn success(return_count: i32) -> Self {
        Self {
            status: 0,
            return_count,
            error_length: 0,
        }
    }

    #[must_use]
    pub const fn failure(status: i32, error_length: u32) -> Self {
        Self {
            status,
            return_count: 0,
            error_length,
        }
    }
}

/// Rust dispatcher signature registered with the generic C++ trampoline.
pub type Dispatcher = unsafe extern "C" fn(*mut RawLuaState, *mut c_char, u32) -> DispatchResult;

unsafe extern "C" {
    fn rsgdll_bridge_set_dispatcher(dispatcher: Dispatcher);
    fn rsgdll_bridge_trampoline(state: *mut RawLuaState) -> c_int;
}

/// Replaces the process-wide dispatcher used by the C++ trampoline.
pub fn set_dispatcher(dispatcher: Dispatcher) {
    // SAFETY: function pointer has the exact C ABI expected by the bridge and
    // remains valid for the process lifetime.
    unsafe { rsgdll_bridge_set_dispatcher(dispatcher) };
}

/// Returns the one generic Lua callback implemented by the C++ firewall.
#[must_use]
pub fn trampoline() -> LuaCFunction {
    rsgdll_bridge_trampoline
}

const _: () = assert!(std::mem::size_of::<DispatchResult>() == 12);
