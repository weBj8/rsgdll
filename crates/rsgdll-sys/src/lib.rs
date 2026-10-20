//! Raw FFI for rsgdll: Lua C API fn table, GMod C++ vtables, engine library loader, tier0.
//! Nothing here is safe to use directly; see `rsgdll-core`.

pub mod gmod;
pub mod loader;
pub mod lua;
pub mod tier0;
#[rustfmt::skip]
pub mod vtables;

use std::sync::OnceLock;

pub use lua::{LuaApi, lua_State};

/// Base names of the GMod Lua library, tried in order (`lua_shared_srv` on old Linux servers).
const LUA_SHARED_NAMES: &[&str] = &["lua_shared", "lua_shared_srv"];

/// The resolved library plus its fn table. Lives for the DLL lifetime.
pub struct Loaded {
    pub lib: libloading::Library,
    pub api: LuaApi,
}

static LUA: OnceLock<Result<Loaded, String>> = OnceLock::new();

/// Resolves `lua_shared` once per DLL (spec §6.1a). Missing symbol = clear error.
pub fn lua_shared() -> Result<&'static Loaded, &'static str> {
    LUA.get_or_init(|| {
        LUA_SHARED_NAMES
            .iter()
            .find_map(|n| loader::find_module(n))
            .ok_or_else(|| "lua_shared is not loaded in this process".to_owned())
            // SAFETY: lua_shared exports the Lua 5.1 C API.
            .and_then(|lib| unsafe { LuaApi::load(&lib) }.map(|api| Loaded { lib, api }))
    })
    .as_ref()
    .map_err(String::as_str)
}

/// Test/runtime hook: use an explicit library (e.g. stock LuaJIT) instead of `lua_shared`.
/// Has no effect if the table is already initialised.
///
/// # Safety
/// `lib` must export the Lua 5.1 C API with the declared signatures.
pub unsafe fn init_with(lib: libloading::Library) -> Result<&'static Loaded, &'static str> {
    LUA.get_or_init(|| unsafe { LuaApi::load(&lib) }.map(|api| Loaded { lib, api }))
        .as_ref()
        .map_err(String::as_str)
}

/// Shortcut used by the safe layer after `lua_shared()` succeeded.
pub fn api() -> &'static LuaApi {
    match LUA.get() {
        Some(Ok(l)) => &l.api,
        _ => panic!("rsgdll-sys: Lua API used before load"),
    }
}
