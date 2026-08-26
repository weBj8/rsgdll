//! Minimal C++ Lua error firewall.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_int};

use rsgdll_platform::__private::{LuaCFunction, RawLuaState};

/// Capacity of the stack-owned error buffer supplied by the C++ trampoline.
pub const ERROR_BUFFER_CAPACITY: u32 = 4096;
/// Maximum number of Lua return values staged outside the Lua stack.
pub const RETURN_SLOT_CAPACITY: usize = 16;
/// Capacity of copied string return data.
pub const RETURN_BYTE_CAPACITY: usize = 4096;

pub const RETURN_NIL: u32 = 0;
pub const RETURN_BOOL: u32 = 1;
pub const RETURN_NUMBER: u32 = 2;
pub const RETURN_STRING: u32 = 3;

/// One POD Lua return value written by Rust and consumed by C++.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ReturnSlot {
    pub tag: u32,
    pub offset: u32,
    pub length: u32,
    pub reserved: u32,
    pub number: f64,
}

/// C++-owned staging storage filled before Rust returns.
#[repr(C)]
pub struct ReturnBuffer {
    pub slots: [ReturnSlot; RETURN_SLOT_CAPACITY],
    pub bytes: [u8; RETURN_BYTE_CAPACITY],
}

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
pub type Dispatcher =
    unsafe extern "C" fn(*mut RawLuaState, *mut c_char, u32, *mut ReturnBuffer) -> DispatchResult;

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
const _: () = assert!(std::mem::size_of::<ReturnSlot>() == 24);
const _: () = assert!(std::mem::size_of::<ReturnBuffer>() == 4480);
