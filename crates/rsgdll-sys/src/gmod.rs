//! GMod C++ interface objects (spec §5.2). Vtable layouts live in [`crate::vtables`].

use core::ffi::{c_char, c_int, c_void};

use crate::lua::lua_State;
use crate::vtables::{
    ILuaBaseVTable, ILuaInterfaceVTable, ILuaSharedVTable, ILuaThreadedCallVTable,
};

/// Calling convention of C++ virtual methods: `thiscall` on Windows x86, C elsewhere.
#[cfg(all(windows, target_arch = "x86"))]
#[macro_export]
#[doc(hidden)]
macro_rules! vfn {
    (($($a:ty),*) $(-> $r:ty)?) => { unsafe extern "thiscall-unwind" fn($($a),*) $(-> $r)? };
}

/// Calling convention of C++ virtual methods: `thiscall` on Windows x86, C elsewhere.
#[cfg(not(all(windows, target_arch = "x86")))]
#[macro_export]
#[doc(hidden)]
macro_rules! vfn {
    (($($a:ty),*) $(-> $r:ty)?) => { unsafe extern "C-unwind" fn($($a),*) $(-> $r)? };
}

/// A C++ object: vtable pointer first.
#[repr(C)]
pub struct Object<V> {
    pub vtable: *const V,
}

pub type ILuaBase = Object<ILuaBaseVTable>;
pub type ILuaShared = Object<ILuaSharedVTable>;
pub type ILuaThreadedCall = Object<ILuaThreadedCallVTable>;

/// `ILuaInterface : ILuaBase`; its vtable starts with the `ILuaBase` slots.
pub type ILuaInterface = Object<ILuaInterfaceVTable>;

impl<V> Object<V> {
    /// # Safety
    /// `self` must be a live object whose vtable has layout `V`.
    pub unsafe fn vt(&self) -> &V {
        unsafe { &*self.vtable }
    }
}

/// Header size before `luabase` in GMod's `lua_State` (garrysmod_common `Interface.h`).
#[cfg(target_pointer_width = "32")]
pub const LUA_STATE_HEADER: usize = 48 + 22;
#[cfg(target_pointer_width = "64")]
pub const LUA_STATE_HEADER: usize = 92 + 22;

/// Mirror of GMod's `lua_State` struct, used only for the field offset.
#[repr(C)]
pub struct GmodLuaState {
    pub _header: [u8; LUA_STATE_HEADER],
    pub luabase: *mut ILuaBase,
}

// Offsets as compiled by garrysmod_common (header + pointer alignment padding).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::offset_of!(GmodLuaState, luabase) == 120);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::offset_of!(GmodLuaState, luabase) == 72);

/// Reads `L->luabase`. Only valid on the main thread of a GMod state (spec §6.1a).
///
/// # Safety
/// `l` must be a GMod main-thread `lua_State`.
pub unsafe fn luabase(l: *mut lua_State) -> *mut ILuaBase {
    unsafe { (*l.cast::<GmodLuaState>()).luabase }
}

/// `CreateInterface` signature (sourcesdk `interface.h`).
pub type CreateInterfaceFn = unsafe extern "C-unwind" fn(*const c_char, *mut c_int) -> *mut c_void;
pub const IFACE_OK: c_int = 0;
pub const IFACE_FAILED: c_int = 1;
pub const LUASHARED_INTERFACE: &str = "LUASHARED003";

/// `GarrysMod::Lua::Type` ids (GMod extends Lua's).
pub mod types {
    use core::ffi::c_int;
    pub const ENTITY: c_int = 9;
    pub const VECTOR: c_int = 10;
    pub const ANGLE: c_int = 11;
    pub const PHYSOBJ: c_int = 12;
    pub const DAMAGEINFO: c_int = 16;
    pub const MOVEDATA: c_int = 18;
    pub const USERCMD: c_int = 20;
    pub const PANEL: c_int = 22;
    pub const CONVAR: c_int = 27;
}

/// `ILuaShared::GetLuaInterface` realm ids.
pub mod realm {
    pub const CLIENT: u8 = 0;
    pub const SERVER: u8 = 1;
    pub const MENU: u8 = 2;
}

/// `ILuaBase::PushSpecial` ids.
pub mod special {
    use core::ffi::c_int;
    pub const GLOB: c_int = 0;
    pub const ENV: c_int = 1;
    pub const REG: c_int = 2;
}
