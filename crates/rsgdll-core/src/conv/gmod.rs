//! GMod value types (spec §6.2): Vector/Angle via `ILuaBase` push/get (main stack +
//! `lua_xmove` from coroutines, spec §6.1a), Color with GMod's metatable, Entity/Player refs.

use core::ffi::{c_int, c_void};
use std::marker::PhantomData;

use rsgdll_sys::gmod::{ILuaBase, types};
use rsgdll_sys::lua::*;

use super::table::{expect_table, field, push_field};
use super::{FromArg, FromLua, IntoLua, Stage, lua_err, mismatch};
use crate::error::{ConvError, Error, Result};
use crate::lua::Lua;
use crate::protected::Op;
use crate::refs::LuaRef;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Angle {
    pub p: f32,
    pub y: f32,
    pub r: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }
}

/// `ILuaBase*` of this state's main thread (only in GMod).
pub fn luabase(lua: &Lua) -> Option<*mut ILuaBase> {
    lua.runtime()
        .ext::<Option<usize>>("luabase", || None)
        .map(|p| p as *mut ILuaBase)
}

#[repr(C)]
struct VecOp {
    base: *mut ILuaBase,
    v: [f32; 3],
    angle: bool,
    ty: c_int,
}

/// Raw protected step on the *main* stack: push Vector/Angle (allocates, may raise).
unsafe extern "C-unwind" fn push_vec(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(op) = data.cast::<VecOp>().as_ref() {
            let vt = (*op.base).vt();
            match op.angle {
                true => (vt.push_angle)(op.base.cast(), op.v.as_ptr().cast()),
                false => (vt.push_vector)(op.base.cast(), op.v.as_ptr().cast()),
            }
        }
    }
}

/// Raw step on the main stack: type-check top, copy 3 floats, pop.
unsafe extern "C-unwind" fn get_vec(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(op) = data.cast::<VecOp>().as_mut() {
            let vt = (*op.base).vt();
            op.ty = (vt.get_type)(op.base.cast(), -1);
            let want = if op.angle {
                types::ANGLE
            } else {
                types::VECTOR
            };
            if op.ty == want {
                let p = match op.angle {
                    true => (vt.get_angle)(op.base.cast(), -1),
                    false => (vt.get_vector)(op.base.cast(), -1),
                };
                op.v = *p;
            }
            (vt.pop)(op.base.cast(), 1);
        }
    }
}

fn need_base(lua: &Lua) -> Result<*mut ILuaBase, ConvError> {
    luabase(lua).ok_or_else(|| ConvError::msg("Vector/Angle need GMod's ILuaBase (not available)"))
}

/// Stages a Vector/Angle push targeting `L`, via the main stack when in a coroutine.
fn stage_vec(s: &mut Stage<'_>, v: [f32; 3], angle: bool) -> Result<(), ConvError> {
    let base = need_base(s.lua)?;
    let op = Box::new(VecOp {
        base,
        v,
        angle,
        ty: 0,
    });
    let p = (&*op as *const VecOp).cast_mut().cast::<c_void>();
    s.keep_box(op);
    let (main, cur) = (s.lua.main_thread(), s.lua.raw());
    match main == cur {
        true => s.op(Op::Raw(push_vec, p), 1),
        false => s
            .op(Op::CheckStackOf(main, 1), 0)
            .op(Op::Raw(push_vec, p), 0)
            .op(Op::XMove(main, cur, 1), 1),
    };
    Ok(())
}

fn read_vec(lua: &Lua, idx: c_int, angle: bool) -> Result<[f32; 3], ConvError> {
    let base = need_base(lua)?;
    let mut op = VecOp {
        base,
        v: [0.0; 3],
        angle,
        ty: -1,
    };
    let p = crate::state::light_mut(&mut op).cast::<c_void>();
    let (main, cur) = (lua.main_thread(), lua.raw());
    let ops: Vec<Op> = match main == cur {
        true => vec![Op::PushValue(1), Op::Raw(get_vec, p)],
        false => vec![
            Op::CheckStackOf(main, 1),
            Op::PushValue(1),
            Op::XMove(cur, main, 1),
            Op::Raw(get_vec, p),
        ],
    };
    let g = lua.guard();
    lua.run(&[idx], &ops).map_err(lua_err)?;
    drop(g);
    let want = if angle { "Angle" } else { "Vector" };
    match op.ty == if angle { types::ANGLE } else { types::VECTOR } {
        true => Ok(op.v),
        false => Err(mismatch(lua, idx, want)),
    }
}

impl FromLua for Vector {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        read_vec(lua, idx, false).map(|[x, y, z]| Vector { x, y, z })
    }
}

impl IntoLua for Vector {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        stage_vec(s, [self.x, self.y, self.z], false)
    }
}

impl FromLua for Angle {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        read_vec(lua, idx, true).map(|[p, y, r]| Angle { p, y, r })
    }
}

impl IntoLua for Angle {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        stage_vec(s, [self.p, self.y, self.r], true)
    }
}

/// Registry ref of GMod's `Color` metatable, fetched once per state (absent outside GMod).
fn color_meta(lua: &Lua) -> Option<c_int> {
    let rt = lua.runtime().clone();
    let cell = rt
        .ext::<std::cell::Cell<Option<Option<c_int>>>>("color_meta", || std::cell::Cell::new(None));
    cell.get().unwrap_or_else(|| {
        let g = lua.guard();
        let found = lua
            .global::<super::LuaFunction>("FindMetaTable")
            .ok()
            .and_then(|f| {
                f.call::<_, Option<super::LuaAny>>(lua, "Color")
                    .ok()
                    .flatten()
            })
            .map(|a| a.0.try_clone(lua).ok())
            .and_then(|r| r)
            .map(|r| {
                let id = r.id();
                std::mem::forget(r); // kept for the state lifetime; freed with the registry
                id
            });
        drop(g);
        cell.set(Some(found));
        found
    })
}

impl FromLua for Color {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        let t = expect_table(lua, idx).map_err(|_| mismatch(lua, idx, "Color"))?;
        Ok(Color {
            r: field(lua, t, "r")?,
            g: field(lua, t, "g")?,
            b: field(lua, t, "b")?,
            a: field::<Option<u8>>(lua, t, "a")?.unwrap_or(255),
        })
    }
}

impl IntoLua for Color {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        let meta = color_meta(s.lua);
        s.table(0, 4);
        [("r", self.r), ("g", self.g), ("b", self.b), ("a", self.a)]
            .into_iter()
            .try_for_each(|(k, v)| push_field(s, k, v))?;
        if let Some(m) = meta {
            s.op(Op::PushRef(m), 1).op(Op::SetMetatable(-2), -1);
        }
        Ok(())
    }
}

/// Calls `slot:method()` protected and reads the first result raw.
fn method_bool(lua: &Lua, idx: c_int, method: &'static core::ffi::CStr) -> Result<bool, ConvError> {
    let g = lua.guard();
    let ops = [
        Op::PushValue(1),
        Op::GetField(-1, method.as_ptr()),
        Op::Insert(-2),
        Op::Call(1, 1),
        Op::Read(-1, 0),
    ];
    let [r] = lua.read_ops::<1>(&[idx], &ops)?;
    drop(g);
    Ok(r.tag == LUA_TBOOLEAN && r.num != 0.0)
}

/// GMod type id of a slot via `ILuaBase::GetType` (falls back to `lua_type`).
fn gmod_type(lua: &Lua, idx: c_int) -> c_int {
    let raw = lua.read(idx).tag;
    match (raw, luabase(lua), lua.is_main_thread()) {
        // SAFETY: GetType on the main stack only reads the slot (no allocation).
        (LUA_TUSERDATA, Some(b), true) => unsafe { ((*b).vt().get_type)(b.cast(), lua.abs(idx)) },
        (t, _, _) => t,
    }
}

fn entity_slot(lua: &Lua, idx: c_int, player: bool) -> Result<Option<c_int>, ConvError> {
    let idx = lua.abs(idx);
    let what = if player { "Player" } else { "Entity" };
    match (lua.read(idx).tag, gmod_type(lua, idx)) {
        (LUA_TNIL | LUA_TNONE, _) => Ok(None),
        (LUA_TUSERDATA, t) if t == types::ENTITY || t == LUA_TUSERDATA => {
            let valid = method_bool(lua, idx, c"IsValid")?;
            match (valid, player) {
                (false, _) => Ok(None), // NULL entity
                (true, true) if !method_bool(lua, idx, c"IsPlayer")? => {
                    Err(mismatch(lua, idx, what))
                }
                (true, _) => Ok(Some(idx)),
            }
        }
        _ => Err(mismatch(lua, idx, what)),
    }
}

macro_rules! entity_types {
    ($($r:ident, $o:ident, $player:expr;)*) => {$(
        /// Borrow of an entity argument slot (no registry ref).
        #[derive(Clone, Copy)]
        pub struct $r<'a> { lua: &'a Lua, idx: c_int, _p: PhantomData<&'a ()> }

        impl<'a> $r<'a> {
            pub fn index(&self) -> c_int { self.idx }
            /// Owned handle for storing beyond the call.
            pub fn to_owned(&self) -> Result<$o> {
                LuaRef::new(self.lua, self.idx).map(|r| $o(r))
            }
            /// Calls `ent:Method(args...)`.
            pub fn call<A: super::IntoLuaMulti, R: super::FromLuaMulti>(&self, method: &str, args: A) -> Result<R> {
                call_method(self.lua, self.idx, method, args)
            }
        }

        impl<'a> FromArg<'a> for Option<$r<'a>> {
            const OPTIONAL: bool = true;
            fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
                entity_slot(lua, idx, $player).map(|o| o.map(|idx| $r { lua, idx, _p: PhantomData }))
            }
        }

        impl<'a> FromArg<'a> for $r<'a> {
            fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
                entity_slot(lua, idx, $player)?
                    .map(|idx| $r { lua, idx, _p: PhantomData })
                    .ok_or_else(|| ConvError::new(stringify!($o), "NULL entity"))
            }
        }

        impl IntoLua for $r<'_> {
            fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
                let r = LuaRef::new(self.lua, self.idx).map_err(lua_err)?;
                s.reference(r).map(|_| ())
            }
        }

        /// Owned entity handle (registry ref); `IsValid` is checked on each use.
        pub struct $o(pub LuaRef);

        impl $o {
            pub fn is_valid(&self, lua: &Lua) -> Result<bool> {
                let g = lua.guard();
                self.0.push(lua)?;
                let r = method_bool(lua, -1, c"IsValid").map_err(Error::from);
                drop(g);
                r
            }
            pub fn call<A: super::IntoLuaMulti, R: super::FromLuaMulti>(&self, lua: &Lua, method: &str, args: A) -> Result<R> {
                match self.is_valid(lua)? {
                    false => Err(crate::anyhow!("{} is no longer valid", stringify!($o))),
                    true => {
                        let g = lua.guard();
                        self.0.push(lua)?;
                        let r = call_method(lua, -1, method, args);
                        drop(g);
                        r
                    }
                }
            }
        }

        impl IntoLua for &$o {
            fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
                s.reference_id(&self.0).map(|_| ())
            }
        }
    )*};
}

entity_types! {
    EntityRef, Entity, false;
    PlayerRef, Player, true;
}

/// `obj:method(args...)` on the value at `idx`.
pub fn call_method<A: super::IntoLuaMulti, R: super::FromLuaMulti>(
    lua: &Lua,
    idx: c_int,
    method: &str,
    args: A,
) -> Result<R> {
    let idx = lua.abs(idx);
    let g = lua.guard();
    let name = method.to_owned();
    let mut s = Stage::with_inputs(lua, &[idx]);
    s.op(Op::PushValue(1), 1)
        .op(Op::PushStr(name.as_ptr(), name.len()), 1)
        .op(Op::GetTable(-2), 0)
        .op(Op::Insert(-2), 0);
    let n = args.into_lua_multi(&mut s)?;
    s.op(Op::Call(n + 1, LUA_MULTRET), -n - 2);
    let r = s
        .finish()
        .and_then(|(first, count)| R::from_lua_multi(lua, first, count).map_err(Error::from));
    drop(g);
    r
}

impl Stage<'_> {
    /// Keeps an arbitrary box alive until the stage runs.
    pub fn keep_box<T: 'static>(&mut self, b: Box<T>) {
        self.extra.push(b);
    }
}
