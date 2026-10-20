//! `LocalLua` (spec §10.2): `!Send` handle for local futures; checks the generation.

use crate::chunk::AsLua;
use crate::error::{Error, Result};
use crate::lua::Lua;

pub struct LocalLua {
    lua: Lua,
}

impl LocalLua {
    /// Always refers to the main thread (never stores a coroutine pointer, spec §6.1a).
    pub fn new(lua: &Lua) -> Self {
        Self {
            lua: Lua::with_runtime(lua.main_thread(), lua.runtime().clone()),
        }
    }

    /// The live `Lua`, or `StateClosed` after `gmod13_close`.
    pub fn lua(&self) -> Result<&Lua> {
        crate::state::is_open(self.lua.generation())
            .then_some(&self.lua)
            .ok_or_else(Error::state_closed)
    }

    pub fn handle(&self) -> Result<super::LuaHandle> {
        self.lua().map(Lua::handle)
    }
}

impl AsLua for LocalLua {
    fn as_lua(&self) -> Result<&Lua> {
        self.lua()
    }
}
