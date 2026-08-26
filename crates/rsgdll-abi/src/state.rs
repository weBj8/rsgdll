use core::mem::{align_of, offset_of, size_of};

use crate::RawLuaBase;

const OPAQUE_HEADER_SIZE: usize = 92 + 22;
const POINTER_ALIGNMENT_PADDING: usize = 6;

/// Handwritten `lua_State` prefix passed to Garry's Mod binary modules.
///
/// Remaining Lua internals are deliberately opaque. This is not LuaJIT's
/// complete `lua_State`.
#[repr(C)]
pub struct RawLuaState {
    opaque_header: [u8; OPAQUE_HEADER_SIZE],
    alignment_padding: [u8; POINTER_ALIGNMENT_PADDING],
    lua_base: *mut RawLuaBase,
}

/// Offset of `lua_State::luabase` on the header-defined Linux x86_64 target.
pub const RAW_LUA_BASE_OFFSET: usize = offset_of!(RawLuaState, lua_base);

const _: () = assert!(RAW_LUA_BASE_OFFSET == 120);
const _: () = assert!(size_of::<RawLuaState>() == 128);
const _: () = assert!(align_of::<RawLuaState>() == 8);

impl RawLuaState {
    /// Reads Garry's Mod's raw Lua interface pointer.
    ///
    /// # Safety
    ///
    /// `state` must point to a live Garry's Mod `lua_State` matching the pinned
    /// Linux x86_64 layout. The returned pointer remains foreign-owned.
    #[must_use]
    pub unsafe fn lua_base(state: *mut Self) -> *mut RawLuaBase {
        // SAFETY: [UB categories 3, 6, 8] The caller guarantees `state` is
        // live, aligned, and uses the pinned foreign layout.
        unsafe { (*state).lua_base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_base_field_matches_pinned_linux_x86_64_layout() {
        // Given: the handwritten current community-header layout.
        // When: Rust computes its field offset and total size.
        // Then: both match the pinned C++ layout including alignment padding.
        assert_eq!(RAW_LUA_BASE_OFFSET, 120);
        assert_eq!(size_of::<RawLuaState>(), 128);
        assert_eq!(align_of::<RawLuaState>(), 8);
    }
}
