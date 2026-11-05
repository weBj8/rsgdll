//! Lua 5.1 / LuaJIT C API: types, constants and the runtime-resolved fn table (spec §5.1).
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

/// Opaque Lua state. GMod's `lua_State` has a `luabase` field; see [`crate::gmod::luabase`].
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct lua_State {
    _private: [u8; 0],
}

pub type CFunc = unsafe extern "C-unwind" fn(*mut lua_State) -> c_int;
pub type lua_Number = f64;
pub type lua_Integer = isize;

pub const LUA_REGISTRYINDEX: c_int = -10000;
pub const LUA_ENVIRONINDEX: c_int = -10001;
pub const LUA_GLOBALSINDEX: c_int = -10002;
pub const LUA_MULTRET: c_int = -1;
pub const LUA_MINSTACK: c_int = 20;
pub const LUA_NOREF: c_int = -2;
pub const LUA_REFNIL: c_int = -1;

pub const LUA_OK: c_int = 0;
pub const LUA_YIELD: c_int = 1;
pub const LUA_ERRRUN: c_int = 2;
pub const LUA_ERRSYNTAX: c_int = 3;
pub const LUA_ERRMEM: c_int = 4;
pub const LUA_ERRERR: c_int = 5;

pub const LUA_TNONE: c_int = -1;
pub const LUA_TNIL: c_int = 0;
pub const LUA_TBOOLEAN: c_int = 1;
pub const LUA_TLIGHTUSERDATA: c_int = 2;
pub const LUA_TNUMBER: c_int = 3;
pub const LUA_TSTRING: c_int = 4;
pub const LUA_TTABLE: c_int = 5;
pub const LUA_TFUNCTION: c_int = 6;
pub const LUA_TUSERDATA: c_int = 7;
pub const LUA_TTHREAD: c_int = 8;

pub const fn lua_upvalueindex(i: c_int) -> c_int {
    LUA_GLOBALSINDEX - i
}

/// Declares the fn table: one `unsafe extern "C-unwind"` pointer per bound symbol.
macro_rules! lua_api {
    ($(#[$doc:meta])* $api:ident { $( $name:ident : fn($($arg:ty),*) $(-> $ret:ty)? ;)* }) => {
        $(#[$doc])*
        #[allow(non_snake_case, missing_docs)]
        pub struct $api {
            $(pub $name: unsafe extern "C-unwind" fn($($arg),*) $(-> $ret)?,)*
        }

        impl $api {
            /// Every bound symbol name, for diagnostics and the probe intersection.
            pub const SYMBOLS: &'static [&'static str] = &[$(stringify!($name)),*];

            /// Resolves every symbol; the first missing one fails the whole load.
            ///
            /// # Safety
            /// `lib` must be a Lua 5.1 ABI library whose symbols match the declared signatures.
            pub unsafe fn load(lib: &libloading::Library) -> Result<Self, String> {
                Ok(Self { $($name: unsafe {
                    *lib.get(concat!(stringify!($name), "\0").as_bytes())
                        .map_err(|e| format!("lua_shared: missing symbol `{}`: {e}", stringify!($name)))?
                },)* })
            }
        }
    };
}

lua_api! {
    /// Lua C API fn pointers resolved from `lua_shared` (or a stock LuaJIT in tests).
    LuaApi {
    lua_gettop: fn(*mut lua_State) -> c_int;
    lua_settop: fn(*mut lua_State, c_int);
    lua_pushvalue: fn(*mut lua_State, c_int);
    lua_remove: fn(*mut lua_State, c_int);
    lua_insert: fn(*mut lua_State, c_int);
    lua_replace: fn(*mut lua_State, c_int);
    lua_checkstack: fn(*mut lua_State, c_int) -> c_int;
    lua_xmove: fn(*mut lua_State, *mut lua_State, c_int);
    lua_type: fn(*mut lua_State, c_int) -> c_int;
    lua_typename: fn(*mut lua_State, c_int) -> *const c_char;
    lua_isnumber: fn(*mut lua_State, c_int) -> c_int;
    lua_isstring: fn(*mut lua_State, c_int) -> c_int;
    lua_iscfunction: fn(*mut lua_State, c_int) -> c_int;
    lua_isuserdata: fn(*mut lua_State, c_int) -> c_int;
    lua_rawequal: fn(*mut lua_State, c_int, c_int) -> c_int;
    lua_equal: fn(*mut lua_State, c_int, c_int) -> c_int;
    lua_lessthan: fn(*mut lua_State, c_int, c_int) -> c_int;
    lua_tonumber: fn(*mut lua_State, c_int) -> lua_Number;
    lua_tointeger: fn(*mut lua_State, c_int) -> lua_Integer;
    lua_toboolean: fn(*mut lua_State, c_int) -> c_int;
    lua_tolstring: fn(*mut lua_State, c_int, *mut usize) -> *const c_char;
    lua_objlen: fn(*mut lua_State, c_int) -> usize;
    lua_tocfunction: fn(*mut lua_State, c_int) -> Option<CFunc>;
    lua_touserdata: fn(*mut lua_State, c_int) -> *mut c_void;
    lua_tothread: fn(*mut lua_State, c_int) -> *mut lua_State;
    lua_topointer: fn(*mut lua_State, c_int) -> *const c_void;
    lua_pushnil: fn(*mut lua_State);
    lua_pushnumber: fn(*mut lua_State, lua_Number);
    lua_pushinteger: fn(*mut lua_State, lua_Integer);
    lua_pushlstring: fn(*mut lua_State, *const c_char, usize);
    lua_pushstring: fn(*mut lua_State, *const c_char);
    lua_pushcclosure: fn(*mut lua_State, CFunc, c_int);
    lua_pushboolean: fn(*mut lua_State, c_int);
    lua_pushlightuserdata: fn(*mut lua_State, *mut c_void);
    lua_pushthread: fn(*mut lua_State) -> c_int;
    lua_gettable: fn(*mut lua_State, c_int);
    lua_getfield: fn(*mut lua_State, c_int, *const c_char);
    lua_rawget: fn(*mut lua_State, c_int);
    lua_rawgeti: fn(*mut lua_State, c_int, c_int);
    lua_createtable: fn(*mut lua_State, c_int, c_int);
    lua_newuserdata: fn(*mut lua_State, usize) -> *mut c_void;
    lua_getmetatable: fn(*mut lua_State, c_int) -> c_int;
    lua_getfenv: fn(*mut lua_State, c_int);
    lua_settable: fn(*mut lua_State, c_int);
    lua_setfield: fn(*mut lua_State, c_int, *const c_char);
    lua_rawset: fn(*mut lua_State, c_int);
    lua_rawseti: fn(*mut lua_State, c_int, c_int);
    lua_setmetatable: fn(*mut lua_State, c_int) -> c_int;
    lua_setfenv: fn(*mut lua_State, c_int) -> c_int;
    lua_call: fn(*mut lua_State, c_int, c_int);
    lua_pcall: fn(*mut lua_State, c_int, c_int, c_int) -> c_int;
    lua_cpcall: fn(*mut lua_State, CFunc, *mut c_void) -> c_int;
    lua_error: fn(*mut lua_State) -> c_int;
    lua_next: fn(*mut lua_State, c_int) -> c_int;
    lua_concat: fn(*mut lua_State, c_int);
    lua_status: fn(*mut lua_State) -> c_int;
    lua_gc: fn(*mut lua_State, c_int, c_int) -> c_int;
    lua_close: fn(*mut lua_State);
    lua_newthread: fn(*mut lua_State) -> *mut lua_State;
    luaL_newstate: fn() -> *mut lua_State;
    luaL_openlibs: fn(*mut lua_State);
    luaL_loadbuffer: fn(*mut lua_State, *const c_char, usize, *const c_char) -> c_int;
    luaL_ref: fn(*mut lua_State, c_int) -> c_int;
    luaL_unref: fn(*mut lua_State, c_int, c_int);
    luaL_newmetatable: fn(*mut lua_State, *const c_char) -> c_int;
    luaL_checkudata: fn(*mut lua_State, c_int, *const c_char) -> *mut c_void;
    luaL_traceback: fn(*mut lua_State, *mut lua_State, *const c_char, c_int);
    }
}

// -- Debug API (Lua 5.1 / LuaJIT 2.1 `lua.h`, spec §5.1a) ---------------------------------

/// `LUA_IDSIZE` of Garry's Mod's LuaJIT build (size of [`lua_Debug::short_src`]). Stock LuaJIT
/// uses 60; GMod uses 128 (its `lua_getstack` stores `i_ci` at 0xb8 on x86-64, 0xa4 on x86).
/// A record sized for 128 is also large enough for a stock build, which only writes less of it.
pub const LUA_IDSIZE: usize = 128;

pub const LUA_HOOKCALL: c_int = 0;
pub const LUA_HOOKRET: c_int = 1;
pub const LUA_HOOKLINE: c_int = 2;
pub const LUA_HOOKCOUNT: c_int = 3;
/// Declared by `lua.h` but never delivered by LuaJIT 2.1 (tail calls report nothing extra).
pub const LUA_HOOKTAILRET: c_int = 4;

pub const LUA_MASKCALL: c_int = 1 << LUA_HOOKCALL;
pub const LUA_MASKRET: c_int = 1 << LUA_HOOKRET;
pub const LUA_MASKLINE: c_int = 1 << LUA_HOOKLINE;
pub const LUA_MASKCOUNT: c_int = 1 << LUA_HOOKCOUNT;

/// Activation record, field-for-field `struct lua_Debug` of Lua 5.1 / LuaJIT 2.1.
/// `i_ci` is private to the VM: only pass records filled by `lua_getstack` or a hook.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct lua_Debug {
    pub event: c_int,
    pub name: *const c_char,
    pub namewhat: *const c_char,
    pub what: *const c_char,
    pub source: *const c_char,
    pub currentline: c_int,
    pub nups: c_int,
    pub linedefined: c_int,
    pub lastlinedefined: c_int,
    pub short_src: [c_char; LUA_IDSIZE],
    pub i_ci: c_int,
}

impl Default for lua_Debug {
    fn default() -> Self {
        // SAFETY: integers, null pointers and a zeroed char array are all valid.
        unsafe { core::mem::zeroed() }
    }
}

// Layout probes: `offsetof` on the LuaJIT 2.1 header (gcc -m32/-m64, clang MSVC targets) with
// LUA_IDSIZE=128, cross-checked against the `i_ci` store in GMod's linux32/linux64 lua_shared.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(core::mem::size_of::<lua_Debug>() == 192);
    assert!(core::mem::align_of::<lua_Debug>() == 8);
    assert!(core::mem::offset_of!(lua_Debug, name) == 8);
    assert!(core::mem::offset_of!(lua_Debug, currentline) == 40);
    assert!(core::mem::offset_of!(lua_Debug, short_src) == 56);
    assert!(core::mem::offset_of!(lua_Debug, i_ci) == 184);
};
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(core::mem::size_of::<lua_Debug>() == 168);
    assert!(core::mem::align_of::<lua_Debug>() == 4);
    assert!(core::mem::offset_of!(lua_Debug, name) == 4);
    assert!(core::mem::offset_of!(lua_Debug, currentline) == 20);
    assert!(core::mem::offset_of!(lua_Debug, short_src) == 36);
    assert!(core::mem::offset_of!(lua_Debug, i_ci) == 164);
};

/// `lua_Hook`: `void (*)(lua_State *, lua_Debug *)`, C ABI (cdecl on x86). `None` = no hook.
/// The hook must not unwind into the VM: catch panics (see `rsgdll_core::debug`).
pub type lua_Hook = Option<unsafe extern "C-unwind" fn(*mut lua_State, *mut lua_Debug)>;

lua_api! {
    /// Lua debug API, resolved separately from [`LuaApi`] so modules that never use it keep
    /// loading if a symbol is missing (see `rsgdll_sys::debug_api`).
    LuaDebugApi {
    lua_getstack: fn(*mut lua_State, c_int, *mut lua_Debug) -> c_int;
    lua_getinfo: fn(*mut lua_State, *const c_char, *mut lua_Debug) -> c_int;
    lua_getlocal: fn(*mut lua_State, *const lua_Debug, c_int) -> *const c_char;
    lua_setlocal: fn(*mut lua_State, *const lua_Debug, c_int) -> *const c_char;
    lua_getupvalue: fn(*mut lua_State, c_int, c_int) -> *const c_char;
    lua_setupvalue: fn(*mut lua_State, c_int, c_int) -> *const c_char;
    lua_sethook: fn(*mut lua_State, lua_Hook, c_int, c_int) -> c_int;
    lua_gethook: fn(*mut lua_State) -> lua_Hook;
    lua_gethookmask: fn(*mut lua_State) -> c_int;
    lua_gethookcount: fn(*mut lua_State) -> c_int;
    }
}
