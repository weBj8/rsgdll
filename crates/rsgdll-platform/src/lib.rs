//! Target-specific behavior shared by higher-level framework crates.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(any(
    all(target_os = "linux", target_env = "gnu", target_arch = "x86"),
    all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86"),
    all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"),
)))]
compile_error!(
    "rsgdll-platform supports only GNU Linux and MSVC Windows on x86 or x86_64; \
     this target has no header-defined ABI description"
);

/// Operating system selected by target configuration.
#[cfg(target_os = "linux")]
pub const TARGET_OS: &str = "linux";
/// Operating system selected by target configuration.
#[cfg(target_os = "windows")]
pub const TARGET_OS: &str = "windows";

/// CPU architecture selected by target configuration.
#[cfg(target_arch = "x86")]
pub const TARGET_ARCH: &str = "x86";
/// CPU architecture selected by target configuration.
#[cfg(target_arch = "x86_64")]
pub const TARGET_ARCH: &str = "x86_64";

/// Offset of Garry's Mod's `ILuaBase` pointer inside the raw Lua state.
pub const LUA_BASE_OFFSET: usize = rsgdll_abi::RAW_LUA_BASE_OFFSET;

/// Whether the selected ABI passed a real-GMod GLuaTest gate.
pub const RUNTIME_ABI_VERIFIED: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

/// Internal ABI plumbing for higher-level workspace crates.
#[doc(hidden)]
pub mod __private {
    pub use rsgdll_abi::{
        LUA_DEBUG_SHORT_SOURCE_CAPACITY, LUA_HOOK_CALL, LUA_HOOK_COUNT, LUA_HOOK_LINE,
        LUA_HOOK_RETURN, LUA_HOOK_TAIL_RETURN, LUA_MASK_CALL, LUA_MASK_COUNT, LUA_MASK_LINE,
        LUA_MASK_RETURN, LuaCFunction, LuaGetHook, LuaGetHookCount, LuaGetHookMask, LuaGetInfo,
        LuaGetLocal, LuaGetStack, LuaGetUpvalue, LuaHook, LuaSetHook, LuaSetLocal, LuaSetUpvalue,
        LuaType, RSGDLL_ABI_CREATE_META_TABLE_SLOT, RSGDLL_ABI_CREATE_TABLE_SLOT,
        RSGDLL_ABI_GET_TYPE_SLOT, RSGDLL_ABI_LUA_BASE_OFFSET, RSGDLL_ABI_NEW_USERDATA_SLOT,
        RSGDLL_ABI_NEXT_SLOT, RSGDLL_ABI_PCALL_SLOT, RSGDLL_ABI_POP_SLOT,
        RSGDLL_ABI_PUSH_BOOL_SLOT, RSGDLL_ABI_PUSH_CLOSURE_SLOT, RSGDLL_ABI_PUSH_META_TABLE_SLOT,
        RSGDLL_ABI_PUSH_NIL_SLOT, RSGDLL_ABI_PUSH_NUMBER_SLOT, RSGDLL_ABI_PUSH_SLOT,
        RSGDLL_ABI_PUSH_SPECIAL_SLOT, RSGDLL_ABI_PUSH_STRING_SLOT, RSGDLL_ABI_RAW_GET_SLOT,
        RSGDLL_ABI_RAW_SET_SLOT, RSGDLL_ABI_REFERENCE_CREATE_SLOT, RSGDLL_ABI_REFERENCE_FREE_SLOT,
        RSGDLL_ABI_REFERENCE_PUSH_SLOT, RSGDLL_ABI_REMOVE_SLOT, RSGDLL_ABI_SET_META_TABLE_SLOT,
        RSGDLL_ABI_SET_STATE_SLOT, RSGDLL_ABI_SET_USER_TYPE_SLOT, RSGDLL_ABI_THROW_ERROR_SLOT,
        RSGDLL_ABI_TOP_SLOT, RawLuaBase, RawLuaDebug, RawLuaState, RawUserData,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_target_matches_header_defined_layout() {
        // Given: one target admitted by compile-time gating.
        // When: platform metadata is inspected.
        // Then: its exact header-defined layout is selected.
        assert!(matches!(TARGET_OS, "linux" | "windows"));
        assert!(matches!(TARGET_ARCH, "x86" | "x86_64"));
        assert_eq!(
            LUA_BASE_OFFSET,
            if cfg!(target_arch = "x86") { 72 } else { 120 }
        );
        assert_eq!(
            RUNTIME_ABI_VERIFIED,
            cfg!(all(target_os = "linux", target_arch = "x86_64"))
        );
    }
}
