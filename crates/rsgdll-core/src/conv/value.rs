//! Dynamic values (spec §6.2): `LuaValue` enum and `LuaAny` (registry ref of anything).

use core::ffi::c_int;

use rsgdll_sys::lua::*;

use super::func::LuaFunction;
use super::table::LuaTable;
use super::{FromLua, IntoLua, Stage, lua_err};
use crate::error::ConvError;
use crate::lua::Lua;
use crate::refs::LuaRef;

/// Any Lua value; reference types are held by registry ref.
pub enum LuaValue {
    Nil,
    Bool(bool),
    Number(f64),
    /// Raw bytes (Lua strings are not necessarily UTF-8).
    String(Vec<u8>),
    Table(LuaTable),
    Function(LuaFunction),
    /// Userdata, thread, light userdata.
    Other(LuaAny),
}

impl std::fmt::Debug for LuaValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LuaValue::Nil => f.write_str("nil"),
            LuaValue::Bool(b) => write!(f, "{b}"),
            LuaValue::Number(n) => write!(f, "{n}"),
            LuaValue::String(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
            LuaValue::Table(_) => f.write_str("table"),
            LuaValue::Function(_) => f.write_str("function"),
            LuaValue::Other(_) => f.write_str("userdata"),
        }
    }
}

impl LuaValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            LuaValue::String(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            LuaValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, LuaValue::Nil)
    }
}

impl FromLua for LuaValue {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        let r = lua.read(idx);
        Ok(match r.tag {
            LUA_TNIL | LUA_TNONE => LuaValue::Nil,
            LUA_TBOOLEAN => LuaValue::Bool(r.num != 0.0),
            LUA_TNUMBER => LuaValue::Number(r.num),
            LUA_TSTRING => LuaValue::String(lua.raw_bytes(idx).map(<[u8]>::to_vec).unwrap_or_default()),
            LUA_TTABLE => LuaValue::Table(LuaTable::from_lua(lua, idx)?),
            LUA_TFUNCTION => LuaValue::Function(LuaFunction::from_lua(lua, idx)?),
            _ => LuaValue::Other(LuaAny::from_lua(lua, idx)?),
        })
    }
}

impl IntoLua for LuaValue {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        match self {
            LuaValue::Nil => s.nil().ok(),
            LuaValue::Bool(b) => s.bool(b).ok(),
            LuaValue::Number(n) => s.number(n).ok(),
            LuaValue::String(b) => s.bytes(b).ok(),
            LuaValue::Table(t) => t.into_lua(s),
            LuaValue::Function(f) => f.into_lua(s),
            LuaValue::Other(a) => a.into_lua(s),
        }
    }
}

impl Stage<'_> {
    fn ok(&mut self) -> Result<(), ConvError> {
        Ok(())
    }
}

/// Registry ref to any value.
pub struct LuaAny(pub LuaRef);

impl FromLua for LuaAny {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        LuaRef::new(lua, idx).map(LuaAny).map_err(lua_err)
    }
}

impl IntoLua for LuaAny {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.reference(self.0).map(|_| ())
    }
}

impl IntoLua for &LuaAny {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        let r = self.0.try_clone(s.lua).map_err(lua_err)?;
        s.reference(r).map(|_| ())
    }
}
