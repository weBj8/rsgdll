//! Stock LuaJIT harness (spec §12): one state per test thread.
#![allow(dead_code)]

use rsgdll_core::Lua;
use rsgdll_sys::lua::*;

pub fn api() -> &'static LuaApi {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let names: Vec<String> = std::env::var("RSGDLL_LUAJIT")
            .into_iter()
            .chain(["libluajit-5.1.so.2", "libluajit-5.1.so", "lua51.dll"].map(String::from))
            .collect();
        let lib = names
            .iter()
            // SAFETY: test-only load of a Lua library.
            .find_map(|n| unsafe { libloading::Library::new(n) }.ok())
            .expect("stock LuaJIT not found; set RSGDLL_LUAJIT");
        unsafe { rsgdll_sys::init_with(lib) }.expect("load LuaJIT");
        rsgdll_core::trampoline::install_panic_hook();
    });
    rsgdll_sys::api()
}

/// Fresh state with an rsgdll runtime named `test`.
pub fn state() -> (*mut lua_State, Lua) {
    let a = api();
    unsafe {
        let l = (a.luaL_newstate)();
        (a.luaL_openlibs)(l);
        let lua = rsgdll_core::lua::open_state(l, "test").expect("open");
        (l, lua)
    }
}

/// Runs a chunk via raw pcall; returns Ok(top-of-stack as string) or the error string.
pub fn exec(l: *mut lua_State, src: &str) -> Result<String, String> {
    let a = api();
    unsafe {
        let top = (a.lua_gettop)(l);
        let st = match (a.luaL_loadbuffer)(l, src.as_ptr().cast(), src.len(), c"=test".as_ptr()) {
            LUA_OK => (a.lua_pcall)(l, 0, 1, 0),
            s => s,
        };
        let mut len = 0usize;
        let p = (a.lua_tolstring)(l, -1, &mut len);
        let s = match p.is_null() {
            true => "nil".to_owned(),
            false => {
                String::from_utf8_lossy(std::slice::from_raw_parts(p.cast(), len)).into_owned()
            }
        };
        (a.lua_settop)(l, top);
        if st == LUA_OK { Ok(s) } else { Err(s) }
    }
}

/// Registers a C function as a global.
pub fn set_global(l: *mut lua_State, name: &std::ffi::CStr, f: CFunc) {
    let a = api();
    unsafe {
        (a.lua_pushcclosure)(l, f, 0);
        (a.lua_setfield)(l, LUA_GLOBALSINDEX, name.as_ptr());
    }
}
