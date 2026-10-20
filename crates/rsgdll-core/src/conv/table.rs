//! Table conversions (spec §6.2): sequences, maps, `LuaTable`. Reads honour metamethods
//! (`lua_gettable`) and run protected; Rust-side building happens between ops.

use core::ffi::c_int;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;

use rsgdll_sys::lua::*;

use super::{FromLua, IntoLua, Stage, lua_err, mismatch};
use crate::error::{ConvError, Result};
use crate::lua::Lua;
use crate::protected::{Op, Raw};
use crate::refs::LuaRef;

/// Pushes `t[key]` (key staged by `key_ops`) onto the stack; returns its index.
/// The caller pops it (usually via a `StackGuard`).
pub fn fetch(lua: &Lua, table: c_int, key_ops: &[Op]) -> Result<c_int, ConvError> {
    let ops: Vec<Op> = [Op::CheckStack(8)]
        .into_iter()
        .chain(key_ops.iter().copied())
        .chain([Op::GetTable(1), Op::Remove(1)])
        .collect();
    lua.protect(&[table], &ops, &mut [])
        .map(|(first, _)| first)
        .map_err(lua_err)
}

/// `t[i]` for a sequence index.
pub fn fetch_index(lua: &Lua, table: c_int, i: usize) -> Result<c_int, ConvError> {
    fetch(lua, table, &[Op::PushNumber(i as f64)])
}

/// `t["name"]`.
pub fn fetch_field(lua: &Lua, table: c_int, name: &str) -> Result<c_int, ConvError> {
    fetch(lua, table, &[Op::PushStr(name.as_ptr(), name.len())])
}

/// Converts `t[name]`, adding `name` to the error path.
pub fn field<T: FromLua>(lua: &Lua, table: c_int, name: &str) -> Result<T, ConvError> {
    let g = lua.guard();
    let r = fetch_field(lua, table, name)
        .and_then(|i| T::from_lua(lua, i))
        .map_err(|e| e.at(name));
    drop(g);
    r
}

/// Whether `t[name]` is nil (for `#[lua(default)]`).
pub fn field_is_nil(lua: &Lua, table: c_int, name: &str) -> Result<bool, ConvError> {
    let g = lua.guard();
    let r = fetch_field(lua, table, name).map(|i| matches!(lua.read(i).tag, LUA_TNIL | LUA_TNONE));
    drop(g);
    r
}

pub fn expect_table(lua: &Lua, idx: c_int) -> Result<c_int, ConvError> {
    let idx = lua.abs(idx);
    match lua.read(idx).tag {
        LUA_TTABLE => Ok(idx),
        _ => Err(mismatch(lua, idx, "table")),
    }
}

/// `#t` (`lua_objlen`; tables have no `__len` in Lua 5.1).
pub fn len(lua: &Lua, idx: c_int) -> Result<usize, ConvError> {
    lua.read_ops::<1>(&[idx], &[Op::Len(1, 0), Op::Pop(1)])
        .map(|[r]| r.len)
}

/// Reads `1..=#t`; a nil hole is an error.
pub fn sequence<T: FromLua>(lua: &Lua, idx: c_int) -> Result<Vec<T>, ConvError> {
    let t = expect_table(lua, idx)?;
    (1..=len(lua, t)?)
        .map(|i| {
            let g = lua.guard();
            let r = fetch_index(lua, t, i).and_then(|v| match lua.read(v).tag {
                LUA_TNIL => Err(ConvError::msg(format!("expected sequence, hole at {i}"))),
                _ => T::from_lua(lua, v).map_err(|e| e.at(i.to_string())),
            });
            drop(g);
            r
        })
        .collect()
}

/// Iterates all pairs with `lua_next`, calling `f(key_idx, value_idx)` for each.
/// Keys are converted from a copy so `lua_tolstring` never mutates the iteration key.
pub fn pairs(
    lua: &Lua,
    idx: c_int,
    mut f: impl FnMut(c_int, c_int) -> Result<(), ConvError>,
) -> Result<(), ConvError> {
    let t = expect_table(lua, idx)?;
    let g = lua.guard();
    let a = lua.api();
    // SAFETY: pushnil uses the reserve and never allocates.
    unsafe { (a.lua_pushnil)(lua.raw()) };
    let r = std::iter::repeat(()).try_for_each(|()| {
        let key = lua.top();
        let mut out = [Raw::default()];
        lua.protect(&[t, key], &[Op::Next(1, 0), Op::Remove(1)], &mut out)
            .map_err(lua_err)?;
        // SAFETY: removing the old key copy never raises.
        unsafe { (a.lua_remove)(lua.raw(), key) };
        match out[0].num != 0.0 {
            false => Err(None),
            true => {
                let (k, v) = (lua.top() - 1, lua.top());
                let inner = lua.guard();
                // SAFETY: copy of the key into the reserve slot.
                unsafe { (a.lua_pushvalue)(lua.raw(), k) };
                let res = f(lua.top(), v);
                drop(inner);
                // SAFETY: pop the value, keep the key for lua_next.
                unsafe { (a.lua_settop)(lua.raw(), -2) };
                res.map_err(Some)
            }
        }
    });
    drop(g);
    match r {
        Ok(()) | Err(None) => Ok(()),
        Err(Some(e)) => Err(e),
    }
}

impl<T: FromLua> FromLua for Vec<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        sequence(lua, idx)
    }
}

impl<T: FromLua> FromLua for VecDeque<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        sequence(lua, idx).map(Into::into)
    }
}

impl<T: FromLua, const N: usize> FromLua for [T; N] {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        sequence(lua, idx)?.try_into().map_err(|v: Vec<T>| {
            ConvError::new(
                format!("sequence of length {N}"),
                format!("length {}", v.len()),
            )
        })
    }
}

fn collect_map<K: FromLua, V: FromLua, M: Default + Extend<(K, V)>>(
    lua: &Lua,
    idx: c_int,
) -> Result<M, ConvError> {
    let mut m = M::default();
    pairs(lua, idx, |k, v| {
        let key = K::from_lua(lua, k)?;
        let seg = lua
            .raw_bytes(k)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_else(|| "key".into());
        let val = V::from_lua(lua, v).map_err(|e| e.at(seg))?;
        m.extend([(key, val)]);
        Ok(())
    })
    .map(|()| m)
}

impl<K: FromLua + Eq + Hash, V: FromLua, S: std::hash::BuildHasher + Default> FromLua
    for HashMap<K, V, S>
{
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        collect_map(lua, idx)
    }
}

impl<K: FromLua + Ord, V: FromLua> FromLua for BTreeMap<K, V> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        collect_map(lua, idx)
    }
}

#[cfg(feature = "indexmap")]
impl<K: FromLua + Eq + Hash, V: FromLua> FromLua for indexmap::IndexMap<K, V> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        collect_map(lua, idx)
    }
}

/// Stages a sequence table.
pub fn push_seq<T: IntoLua>(
    s: &mut Stage<'_>,
    items: impl ExactSizeIterator<Item = T>,
) -> Result<(), ConvError> {
    s.table(items.len(), 0);
    items.enumerate().try_for_each(|(i, v)| {
        v.into_lua(s).map_err(|e| e.at((i + 1).to_string()))?;
        s.raw_seti(c_int::try_from(i + 1).unwrap_or(c_int::MAX));
        Ok(())
    })
}

/// Stages a map table.
pub fn push_map<K: IntoLua, V: IntoLua>(
    s: &mut Stage<'_>,
    items: impl ExactSizeIterator<Item = (K, V)>,
) -> Result<(), ConvError> {
    s.table(0, items.len());
    items.into_iter().try_for_each(|(k, v)| {
        k.into_lua(s)?;
        v.into_lua(s)?;
        s.raw_set();
        Ok(())
    })
}

/// Stages `t[name] = v` for the table on top (derive helper).
pub fn push_field<T: IntoLua>(
    s: &mut Stage<'_>,
    name: &'static str,
    v: T,
) -> Result<(), ConvError> {
    s.op(Op::PushStr(name.as_ptr(), name.len()), 1);
    v.into_lua(s).map_err(|e| e.at(name))?;
    s.raw_set();
    Ok(())
}

impl<T: IntoLua> IntoLua for Vec<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_seq(s, self.into_iter())
    }
}

impl<T: IntoLua> IntoLua for VecDeque<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_seq(s, self.into_iter())
    }
}

impl<T: IntoLua, const N: usize> IntoLua for [T; N] {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_seq(s, self.into_iter())
    }
}

impl<K: IntoLua, V: IntoLua, S> IntoLua for HashMap<K, V, S> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_map(s, self.into_iter())
    }
}

impl<K: IntoLua, V: IntoLua> IntoLua for BTreeMap<K, V> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_map(s, self.into_iter())
    }
}

#[cfg(feature = "indexmap")]
impl<K: IntoLua, V: IntoLua> IntoLua for indexmap::IndexMap<K, V> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        push_map(s, self.into_iter())
    }
}

/// A table held by registry ref.
pub struct LuaTable(pub LuaRef);

impl LuaTable {
    /// New empty table.
    pub fn new(lua: &Lua) -> Result<Self> {
        let g = lua.guard();
        let mut s = Stage::new(lua);
        s.table(0, 0);
        s.finish()?;
        let r = LuaRef::new(lua, -1).map(LuaTable);
        drop(g);
        r
    }

    /// The globals table `_G`.
    pub fn globals(lua: &Lua) -> Result<Self> {
        let g = lua.guard();
        lua.run(&[], &[Op::PushGlobals])?;
        let r = LuaRef::new(lua, -1).map(LuaTable);
        drop(g);
        r
    }

    /// `t[k]` (metamethods honoured).
    pub fn get<K: IntoLua, V: FromLua>(&self, lua: &Lua, k: K) -> Result<V> {
        let g = lua.guard();
        let mut s = Stage::new(lua);
        s.reference_id(&self.0)?;
        s.push(k)?;
        s.op(Op::GetTable(-2), 0);
        s.finish()?;
        let r = V::from_lua(lua, -1).map_err(Into::into);
        drop(g);
        r
    }

    /// `t[k] = v` (metamethods honoured).
    pub fn set<K: IntoLua, V: IntoLua>(&self, lua: &Lua, k: K, v: V) -> Result<()> {
        let g = lua.guard();
        let mut s = Stage::new(lua);
        s.reference_id(&self.0)?;
        s.push(k)?;
        s.push(v)?;
        s.op(Op::SetTable(-3), -2);
        let r = s.finish().map(|_| ());
        drop(g);
        r
    }

    pub fn len(&self, lua: &Lua) -> Result<usize> {
        let g = lua.guard();
        self.0.push(lua)?;
        let r = len(lua, -1).map_err(Into::into);
        drop(g);
        r
    }

    pub fn is_empty(&self, lua: &Lua) -> Result<bool> {
        self.len(lua).map(|n| n == 0)
    }

    /// All pairs converted to `(K, V)`.
    pub fn pairs<K: FromLua, V: FromLua>(&self, lua: &Lua) -> Result<Vec<(K, V)>> {
        let g = lua.guard();
        self.0.push(lua)?;
        let mut out = Vec::new();
        let r = pairs(lua, -1, |k, v| {
            out.push((K::from_lua(lua, k)?, V::from_lua(lua, v)?));
            Ok(())
        });
        drop(g);
        r.map(|()| out).map_err(Into::into)
    }

    /// Sequence part `1..=#t`.
    pub fn sequence<V: FromLua>(&self, lua: &Lua) -> Result<Vec<V>> {
        let g = lua.guard();
        self.0.push(lua)?;
        let r = sequence(lua, -1).map_err(Into::into);
        drop(g);
        r
    }
}

impl FromLua for LuaTable {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        expect_table(lua, idx).and_then(|i| LuaRef::new(lua, i).map(LuaTable).map_err(lua_err))
    }
}

impl IntoLua for LuaTable {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.reference(self.0).map(|_| ())
    }
}

impl IntoLua for &LuaTable {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        let r = self.0.try_clone(s.lua).map_err(lua_err)?;
        s.reference(r).map(|_| ())
    }
}
