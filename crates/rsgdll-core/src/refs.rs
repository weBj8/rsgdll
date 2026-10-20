//! Registry refs (spec §6.2): `!Send`, generation-checked, `Drop` defers the unref.

use core::ffi::c_int;
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::lua::Lua;
use crate::protected::{Op, Raw};
use crate::state;

/// A registry reference to any Lua value.
pub struct LuaRef {
    id: c_int,
    generation: u64,
    _not_send: PhantomData<*mut ()>,
}

impl LuaRef {
    /// Refs the value at `idx` (protected: `luaL_ref` may allocate).
    pub fn new(lua: &Lua, idx: c_int) -> Result<Self> {
        let g = lua.guard();
        let mut out = [Raw::default()];
        lua.protect(&[idx], &[Op::PushValue(1), Op::Ref(0)], &mut out)?;
        drop(g);
        Ok(Self {
            id: out[0].num as c_int,
            generation: lua.generation(),
            _not_send: PhantomData,
        })
    }

    pub fn id(&self) -> c_int {
        self.id
    }

    /// Fails with `StateClosed` if the state was closed or this ref belongs to another state.
    pub fn check(&self, lua: &Lua) -> Result<()> {
        (lua.generation() == self.generation && state::is_open(self.generation))
            .then_some(())
            .ok_or_else(Error::state_closed)
    }

    /// Pushes the value (no allocation).
    pub fn push(&self, lua: &Lua) -> Result<()> {
        self.check(lua).map(|()| lua.push_ref(self.id))
    }

    pub fn try_clone(&self, lua: &Lua) -> Result<Self> {
        let g = lua.guard();
        self.push(lua)?;
        let r = Self::new(lua, -1);
        drop(g);
        r
    }
}

impl Drop for LuaRef {
    fn drop(&mut self) {
        state::defer_unref(self.generation, self.id);
    }
}
