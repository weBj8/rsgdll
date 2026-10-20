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
    ($( $name:ident : fn($($arg:ty),*) $(-> $ret:ty)? ;)*) => {
        /// Lua C API fn pointers resolved from `lua_shared` (or a stock LuaJIT in tests).
        #[allow(non_snake_case, missing_docs)]
        pub struct LuaApi {
            $(pub $name: unsafe extern "C-unwind" fn($($arg),*) $(-> $ret)?,)*
        }

        impl LuaApi {
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
