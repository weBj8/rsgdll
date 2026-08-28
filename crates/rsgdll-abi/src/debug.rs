use core::ffi::{c_char, c_int};
use core::mem::{offset_of, size_of};

use crate::RawLuaState;

pub const LUA_HOOK_CALL: c_int = 0;
pub const LUA_HOOK_RETURN: c_int = 1;
pub const LUA_HOOK_LINE: c_int = 2;
pub const LUA_HOOK_COUNT: c_int = 3;
pub const LUA_HOOK_TAIL_RETURN: c_int = 4;

pub const LUA_MASK_CALL: c_int = 1 << LUA_HOOK_CALL;
pub const LUA_MASK_RETURN: c_int = 1 << LUA_HOOK_RETURN;
pub const LUA_MASK_LINE: c_int = 1 << LUA_HOOK_LINE;
pub const LUA_MASK_COUNT: c_int = 1 << LUA_HOOK_COUNT;

pub const LUA_DEBUG_SHORT_SOURCE_CAPACITY: usize = 128;

/// Pinned Garry's Mod LuaJIT activation record.
#[repr(C)]
pub struct RawLuaDebug {
    pub event: c_int,
    pub name: *const c_char,
    pub name_what: *const c_char,
    pub what: *const c_char,
    pub source: *const c_char,
    pub current_line: c_int,
    pub upvalue_count: c_int,
    pub line_defined: c_int,
    pub last_line_defined: c_int,
    pub short_source: [c_char; LUA_DEBUG_SHORT_SOURCE_CAPACITY],
    pub private_call_info: c_int,
}

impl RawLuaDebug {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            event: 0,
            name: core::ptr::null(),
            name_what: core::ptr::null(),
            what: core::ptr::null(),
            source: core::ptr::null(),
            current_line: 0,
            upvalue_count: 0,
            line_defined: 0,
            last_line_defined: 0,
            short_source: [0; LUA_DEBUG_SHORT_SOURCE_CAPACITY],
            private_call_info: 0,
        }
    }
}

pub type LuaHook = Option<unsafe extern "C" fn(*mut RawLuaState, *mut RawLuaDebug)>;
pub type LuaGetStack = unsafe extern "C" fn(*mut RawLuaState, c_int, *mut RawLuaDebug) -> c_int;
pub type LuaGetInfo =
    unsafe extern "C" fn(*mut RawLuaState, *const c_char, *mut RawLuaDebug) -> c_int;
pub type LuaGetLocal =
    unsafe extern "C" fn(*mut RawLuaState, *const RawLuaDebug, c_int) -> *const c_char;
pub type LuaSetLocal =
    unsafe extern "C" fn(*mut RawLuaState, *const RawLuaDebug, c_int) -> *const c_char;
pub type LuaGetUpvalue = unsafe extern "C" fn(*mut RawLuaState, c_int, c_int) -> *const c_char;
pub type LuaSetUpvalue = unsafe extern "C" fn(*mut RawLuaState, c_int, c_int) -> *const c_char;
pub type LuaSetHook = unsafe extern "C" fn(*mut RawLuaState, LuaHook, c_int, c_int) -> c_int;
pub type LuaGetHook = unsafe extern "C" fn(*mut RawLuaState) -> LuaHook;
pub type LuaGetHookMask = unsafe extern "C" fn(*mut RawLuaState) -> c_int;
pub type LuaGetHookCount = unsafe extern "C" fn(*mut RawLuaState) -> c_int;

const _: () = {
    assert!(offset_of!(RawLuaDebug, event) == 0);
    assert!(
        offset_of!(RawLuaDebug, name)
            == if cfg!(target_pointer_width = "64") {
                8
            } else {
                4
            }
    );
    assert!(
        offset_of!(RawLuaDebug, short_source)
            == if cfg!(target_pointer_width = "64") {
                56
            } else {
                36
            }
    );
    assert!(
        size_of::<RawLuaDebug>()
            == if cfg!(target_pointer_width = "64") {
                192
            } else {
                168
            }
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_debug_record_layout_matches_gmod_luajit_headers() {
        assert_eq!(offset_of!(RawLuaDebug, event), 0);
        assert_eq!(
            offset_of!(RawLuaDebug, source),
            if cfg!(target_pointer_width = "64") {
                32
            } else {
                16
            }
        );
        assert_eq!(
            offset_of!(RawLuaDebug, current_line),
            if cfg!(target_pointer_width = "64") {
                40
            } else {
                20
            }
        );
        assert_eq!(
            offset_of!(RawLuaDebug, private_call_info),
            if cfg!(target_pointer_width = "64") {
                184
            } else {
                164
            }
        );
    }

    #[test]
    fn pinned_debug_events_and_masks_match_lua_header() {
        assert_eq!(
            [
                LUA_HOOK_CALL,
                LUA_HOOK_RETURN,
                LUA_HOOK_LINE,
                LUA_HOOK_COUNT,
                LUA_HOOK_TAIL_RETURN,
            ],
            [0, 1, 2, 3, 4]
        );
        assert_eq!(
            [
                LUA_MASK_CALL,
                LUA_MASK_RETURN,
                LUA_MASK_LINE,
                LUA_MASK_COUNT
            ],
            [1, 2, 4, 8]
        );
    }
}
