//! Loads a stock LuaJIT 2.1 (spec §12) and checks the bound fn table works.

use rsgdll_sys::lua::*;

/// Stock LuaJIT path: `RSGDLL_LUAJIT` or common system names.
pub fn luajit() -> libloading::Library {
    let names: Vec<String> = std::env::var("RSGDLL_LUAJIT").into_iter().chain(
        ["libluajit-5.1.so.2", "libluajit-5.1.so", "lua51.dll", "libluajit-5.1.dylib"].map(String::from),
    ).collect();
    names.into_iter()
        // SAFETY: test-only load of a Lua library.
        .find_map(|n| unsafe { libloading::Library::new(&n) }.ok())
        .expect("stock LuaJIT not found; set RSGDLL_LUAJIT")
}

#[test]
fn fn_table_runs_chunk() {
    // SAFETY: LuaJIT exports the Lua 5.1 C API.
    let loaded = unsafe { rsgdll_sys::init_with(luajit()) }.expect("load");
    let a = &loaded.api;
    unsafe {
        let l = (a.luaL_newstate)();
        (a.luaL_openlibs)(l);
        let src = "return 1 + 2";
        assert_eq!((a.luaL_loadbuffer)(l, src.as_ptr().cast(), src.len(), c"=t".as_ptr()), LUA_OK);
        assert_eq!((a.lua_pcall)(l, 0, 1, 0), LUA_OK);
        assert_eq!((a.lua_tonumber)(l, -1), 3.0);
        (a.lua_close)(l);
    }
}

#[test]
fn every_symbol_is_listed() {
    assert!(LuaApi::SYMBOLS.contains(&"luaL_traceback"));
    assert!(rsgdll_sys::loader::candidates("engine").any(|c| c.contains("engine")));
}
