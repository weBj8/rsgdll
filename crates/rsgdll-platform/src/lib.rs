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

/// Runtime verification is intentionally withheld until real GMod E2E runs.
pub const RUNTIME_ABI_VERIFIED: bool = false;

const _: () = assert!(!RUNTIME_ABI_VERIFIED);

/// Internal ABI plumbing for higher-level workspace crates.
#[doc(hidden)]
pub mod __private {
    pub use rsgdll_abi::{LuaCFunction, LuaType, RawLuaBase, RawLuaState};
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
