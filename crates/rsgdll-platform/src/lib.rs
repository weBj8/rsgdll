//! Target-specific behavior shared by higher-level framework crates.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "rsgdll-platform has no reviewed ABI description for this target; \
     only Linux x86_64 is currently header-defined"
);

/// Operating system selected by target configuration.
pub const TARGET_OS: &str = "linux";

/// CPU architecture selected by target configuration.
pub const TARGET_ARCH: &str = "x86_64";

/// Offset of Garry's Mod's `ILuaBase` pointer inside the raw Lua state.
pub const LUA_BASE_OFFSET: usize = rsgdll_abi::RAW_LUA_BASE_OFFSET;

/// The selected ABI passed the Linux x86_64 real-GMod GLuaTest gate.
pub const RUNTIME_ABI_VERIFIED: bool = true;

const _: () = assert!(RUNTIME_ABI_VERIFIED);

/// Internal ABI plumbing for higher-level workspace crates.
#[doc(hidden)]
pub mod __private {
    pub use rsgdll_abi::{
        LuaCFunction, LuaType, RSGDLL_ABI_CREATE_META_TABLE_SLOT, RSGDLL_ABI_CREATE_TABLE_SLOT,
        RSGDLL_ABI_GET_TYPE_SLOT, RSGDLL_ABI_LUA_BASE_OFFSET, RSGDLL_ABI_NEW_USERDATA_SLOT,
        RSGDLL_ABI_NEXT_SLOT, RSGDLL_ABI_PCALL_SLOT, RSGDLL_ABI_POP_SLOT,
        RSGDLL_ABI_PUSH_BOOL_SLOT, RSGDLL_ABI_PUSH_CLOSURE_SLOT, RSGDLL_ABI_PUSH_META_TABLE_SLOT,
        RSGDLL_ABI_PUSH_NIL_SLOT, RSGDLL_ABI_PUSH_NUMBER_SLOT, RSGDLL_ABI_PUSH_SLOT,
        RSGDLL_ABI_PUSH_SPECIAL_SLOT, RSGDLL_ABI_PUSH_STRING_SLOT, RSGDLL_ABI_RAW_GET_SLOT,
        RSGDLL_ABI_RAW_SET_SLOT, RSGDLL_ABI_REFERENCE_CREATE_SLOT, RSGDLL_ABI_REFERENCE_FREE_SLOT,
        RSGDLL_ABI_REFERENCE_PUSH_SLOT, RSGDLL_ABI_REMOVE_SLOT, RSGDLL_ABI_SET_META_TABLE_SLOT,
        RSGDLL_ABI_SET_STATE_SLOT, RSGDLL_ABI_SET_USER_TYPE_SLOT, RSGDLL_ABI_THROW_ERROR_SLOT,
        RSGDLL_ABI_TOP_SLOT, RawLuaBase, RawLuaState, RawUserData,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_x86_64_target_matches_header_defined_layout() {
        // Given: the only target admitted by compile-time gating.
        // When: platform metadata is inspected.
        // Then: its exact header-defined layout is selected.
        assert_eq!(TARGET_OS, "linux");
        assert_eq!(TARGET_ARCH, "x86_64");
        assert_eq!(LUA_BASE_OFFSET, 120);
    }
}
