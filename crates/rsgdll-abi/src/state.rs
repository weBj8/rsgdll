use core::mem::{align_of, offset_of, size_of};

use crate::RawLuaBase;

#[cfg(target_pointer_width = "32")]
const OPAQUE_HEADER_SIZE: usize = 48 + 22;
#[cfg(target_pointer_width = "64")]
const OPAQUE_HEADER_SIZE: usize = 92 + 22;

#[cfg(target_pointer_width = "32")]
const POINTER_ALIGNMENT_PADDING: usize = 2;
#[cfg(target_pointer_width = "64")]
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

/// Offset of `lua_State::luabase` on the selected header-defined target.
pub const RAW_LUA_BASE_OFFSET: usize = offset_of!(RawLuaState, lua_base);

#[doc(hidden)]
pub const RSGDLL_ABI_LUA_BASE_OFFSET: usize = RAW_LUA_BASE_OFFSET;

const _: () = assert!(RAW_LUA_BASE_OFFSET == OPAQUE_HEADER_SIZE + POINTER_ALIGNMENT_PADDING);
const _: () =
    assert!(size_of::<RawLuaState>() == RAW_LUA_BASE_OFFSET + size_of::<*mut RawLuaBase>());
const _: () = assert!(align_of::<RawLuaState>() == align_of::<*mut RawLuaBase>());

impl RawLuaState {
    /// Reads Garry's Mod's raw Lua interface pointer.
    ///
    /// # Safety
    ///
    /// `state` must point to a live Garry's Mod `lua_State` matching the pinned
    /// selected target layout. The returned pointer remains foreign-owned.
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
    fn lua_base_field_matches_pinned_target_layout() {
        // Given: the handwritten current community-header layout.
        // When: Rust computes its field offset and total size.
        // Then: both match the pinned C++ layout including alignment padding.
        assert_eq!(
            RAW_LUA_BASE_OFFSET,
            if cfg!(target_pointer_width = "32") {
                72
            } else {
                120
            }
        );
        assert_eq!(
            size_of::<RawLuaState>(),
            RAW_LUA_BASE_OFFSET + size_of::<*mut RawLuaBase>()
        );
        assert_eq!(align_of::<RawLuaState>(), align_of::<*mut RawLuaBase>());
    }
}
