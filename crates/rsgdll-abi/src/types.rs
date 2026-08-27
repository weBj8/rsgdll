use core::ffi::{c_int, c_void};
use core::mem::{align_of, offset_of, size_of};

use crate::RawLuaState;

/// Raw Lua callback accepted by `ILuaBase`.
pub type LuaCFunction = unsafe extern "C" fn(*mut RawLuaState) -> c_int;

/// Garry's Mod userdata header.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RawUserData {
    pub data: *mut c_void,
    pub lua_type: u8,
}

const _: () = assert!(size_of::<RawUserData>() == 2 * size_of::<*mut c_void>());
const _: () = assert!(align_of::<RawUserData>() == align_of::<*mut c_void>());
const _: () = assert!(offset_of!(RawUserData, lua_type) == size_of::<*mut c_void>());

/// Integer Lua/GMod type tag.
///
/// This is a newtype rather than a Rust enum because foreign code may return
/// unknown future values; materializing an invalid Rust enum would be UB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct LuaType(pub c_int);

impl LuaType {
    pub const NONE: Self = Self(-1);
    pub const NIL: Self = Self(0);
    pub const BOOL: Self = Self(1);
    pub const LIGHT_USER_DATA: Self = Self(2);
    pub const NUMBER: Self = Self(3);
    pub const STRING: Self = Self(4);
    pub const TABLE: Self = Self(5);
    pub const FUNCTION: Self = Self(6);
    pub const USER_DATA: Self = Self(7);
    pub const THREAD: Self = Self(8);
    pub const ENTITY: Self = Self(9);
    pub const VECTOR: Self = Self(10);
    pub const ANGLE: Self = Self(11);
    pub const PHYS_OBJ: Self = Self(12);
    pub const SAVE: Self = Self(13);
    pub const RESTORE: Self = Self(14);
    pub const DAMAGE_INFO: Self = Self(15);
    pub const EFFECT_DATA: Self = Self(16);
    pub const MOVE_DATA: Self = Self(17);
    pub const RECIPIENT_FILTER: Self = Self(18);
    pub const USER_CMD: Self = Self(19);
    pub const SCRIPTED_VEHICLE: Self = Self(20);
    pub const MATERIAL: Self = Self(21);
    pub const PANEL: Self = Self(22);
    pub const PARTICLE: Self = Self(23);
    pub const PARTICLE_EMITTER: Self = Self(24);
    pub const TEXTURE: Self = Self(25);
    pub const USER_MSG: Self = Self(26);
    pub const CON_VAR: Self = Self(27);
    pub const I_MESH: Self = Self(28);
    pub const MATRIX: Self = Self(29);
    pub const SOUND: Self = Self(30);
    pub const PIXEL_VIS_HANDLE: Self = Self(31);
    pub const D_LIGHT: Self = Self(32);
    pub const VIDEO: Self = Self(33);
    pub const FILE: Self = Self(34);
    pub const LOCOMOTION: Self = Self(35);
    pub const PATH: Self = Self(36);
    pub const NAV_AREA: Self = Self(37);
    pub const SOUND_HANDLE: Self = Self(38);
    pub const NAV_LADDER: Self = Self(39);
    pub const PARTICLE_SYSTEM: Self = Self(40);
    pub const PROJECTED_TEXTURE: Self = Self(41);
    pub const PHYS_COLLIDE: Self = Self(42);
    pub const SURFACE_INFO: Self = Self(43);
    pub const COUNT: Self = Self(44);
}

/// Selector accepted by `ILuaBase::PushSpecial`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum SpecialIndex {
    Global = 0,
    Environment = 1,
    Registry = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_userdata_matches_selected_c_layout() {
        // Given: one pointer followed by the one-byte GMod type tag.
        // When: Rust applies C field alignment.
        // Then: layout matches the community C++ header.
        assert_eq!(size_of::<RawUserData>(), 2 * size_of::<*mut c_void>());
        assert_eq!(align_of::<RawUserData>(), align_of::<*mut c_void>());
        assert_eq!(offset_of!(RawUserData, lua_type), size_of::<*mut c_void>());
    }

    #[test]
    fn lua_type_values_match_pinned_types_header() {
        // Given: representative boundary tags from Types.h.
        // When: their integer values are read.
        // Then: default Lua and latest GMod tags retain exact discriminants.
        assert_eq!(LuaType::NONE.0, -1);
        assert_eq!(LuaType::NIL.0, 0);
        assert_eq!(LuaType::ENTITY.0, 9);
        assert_eq!(LuaType::SURFACE_INFO.0, 43);
        assert_eq!(LuaType::COUNT.0, 44);
    }
}
