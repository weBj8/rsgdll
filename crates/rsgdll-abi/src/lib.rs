//! Raw Garry's Mod and Lua ABI definitions.
//!
//! This crate is internal, dependency-free, and intentionally exposes only raw
//! pointers and explicitly unsafe ABI calls. See `docs/abi-reference.md` for
//! the pinned upstream definitions.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(any(
    all(target_os = "linux", target_env = "gnu", target_arch = "x86"),
    all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"),
)))]
compile_error!(
    "rsgdll-abi supports only GNU Linux and MSVC Windows on x86 or x86_64; \
     this target has no header-defined ABI description"
);

mod lua_base;
mod state;
mod types;

pub use lua_base::{
    RSGDLL_ABI_CREATE_META_TABLE_SLOT, RSGDLL_ABI_CREATE_TABLE_SLOT, RSGDLL_ABI_GET_TYPE_SLOT,
    RSGDLL_ABI_NEW_USERDATA_SLOT, RSGDLL_ABI_NEXT_SLOT, RSGDLL_ABI_PCALL_SLOT, RSGDLL_ABI_POP_SLOT,
    RSGDLL_ABI_PUSH_BOOL_SLOT, RSGDLL_ABI_PUSH_CLOSURE_SLOT, RSGDLL_ABI_PUSH_META_TABLE_SLOT,
    RSGDLL_ABI_PUSH_NIL_SLOT, RSGDLL_ABI_PUSH_NUMBER_SLOT, RSGDLL_ABI_PUSH_SLOT,
    RSGDLL_ABI_PUSH_SPECIAL_SLOT, RSGDLL_ABI_PUSH_STRING_SLOT, RSGDLL_ABI_RAW_GET_SLOT,
    RSGDLL_ABI_RAW_SET_SLOT, RSGDLL_ABI_REFERENCE_CREATE_SLOT, RSGDLL_ABI_REFERENCE_FREE_SLOT,
    RSGDLL_ABI_REFERENCE_PUSH_SLOT, RSGDLL_ABI_REMOVE_SLOT, RSGDLL_ABI_SET_META_TABLE_SLOT,
    RSGDLL_ABI_SET_STATE_SLOT, RSGDLL_ABI_SET_USER_TYPE_SLOT, RSGDLL_ABI_THROW_ERROR_SLOT,
    RSGDLL_ABI_TOP_SLOT, RawLuaBase,
};
pub use state::{RAW_LUA_BASE_OFFSET, RSGDLL_ABI_LUA_BASE_OFFSET, RawLuaState};
pub use types::{LuaCFunction, LuaType, RawUserData, SpecialIndex};
