//! Lua <-> Rust value conversion (spec §6.2).
//!
//! - [`IntoLua`] stages pushes into a [`Stage`]; [`Stage::finish`] runs them as one
//!   protected op (one `lua_checkstack` for the exact depth first).
//! - [`FromLua`] produces owned values; [`FromArg`] also allows borrows of a live stack
//!   slot (`&str`, `EntityRef`), used for export/hook arguments only.

pub mod func;
pub mod gmod;
pub mod multi;
pub mod prim;
#[cfg(feature = "serde")]
pub mod serde;
pub mod table;
pub mod userdata;
pub mod value;

use core::ffi::c_int;

use rsgdll_sys::lua::*;

use crate::error::{ConvError, Error, ErrorKind, Result};
use crate::lua::Lua;
use crate::protected::{Op, Raw};
use crate::refs::LuaRef;

/// Converts a Lua value at `idx` into an owned Rust value.
pub trait FromLua: Sized {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError>;
}

/// Argument conversion: may borrow the stack slot for `'a`.
pub trait FromArg<'a>: Sized {
    fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError>;
    /// `true` for `Option<_>`: a missing trailing arg is accepted.
    const OPTIONAL: bool = false;
}

impl<'a, T: FromLua> FromArg<'a> for T {
    fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
        T::from_lua(lua, idx)
    }
}

/// Stages one Lua value (net +1 slot) into `stage`.
pub trait IntoLua {
    fn into_lua(self, stage: &mut Stage<'_>) -> Result<(), ConvError>;
}

/// Batched push buffer. Owns every byte buffer and ref the ops point at until it runs.
pub struct Stage<'l> {
    pub lua: &'l Lua,
    ops: Vec<Op>,
    bufs: Vec<Box<[u8]>>,
    extra: Vec<Box<dyn std::any::Any>>,
    keep: Vec<LuaRef>,
    depth: c_int,
    max: c_int,
    inputs: Vec<c_int>,
}

impl<'l> Stage<'l> {
    pub fn new(lua: &'l Lua) -> Self {
        Self { lua, ops: Vec::new(), bufs: Vec::new(), extra: Vec::new(), keep: Vec::new(), depth: 0, max: 0, inputs: Vec::new() }
    }

    /// Stage whose ops may refer to caller slots `inputs` as frame indices `1..=n`
    /// (ops run in their own frame; absolute caller indices are not visible there).
    pub fn with_inputs(lua: &'l Lua, inputs: &[c_int]) -> Self {
        Self { inputs: inputs.to_vec(), ..Self::new(lua) }
    }

    /// Records `op` with its net stack effect.
    pub fn op(&mut self, op: Op, delta: c_int) -> &mut Self {
        self.ops.push(op);
        self.depth += delta;
        self.max = self.max.max(self.depth);
        self
    }

    pub fn nil(&mut self) -> &mut Self {
        self.op(Op::PushNil, 1)
    }

    pub fn bool(&mut self, b: bool) -> &mut Self {
        self.op(Op::PushBool(b), 1)
    }

    pub fn number(&mut self, n: f64) -> &mut Self {
        self.op(Op::PushNumber(n), 1)
    }

    pub fn bytes(&mut self, b: impl Into<Box<[u8]>>) -> &mut Self {
        let b: Box<[u8]> = b.into();
        let op = Op::PushStr(b.as_ptr(), b.len());
        self.bufs.push(b);
        self.op(op, 1)
    }

    /// Static C string pointer kept alive by the caller (field names from derives are `'static`).
    pub fn cstr(&mut self, s: &'static core::ffi::CStr) -> *const core::ffi::c_char {
        s.as_ptr()
    }

    /// Pushes a ref's value; the ref is kept until the stage runs.
    pub fn reference(&mut self, r: LuaRef) -> Result<&mut Self, ConvError> {
        r.check(self.lua).map_err(|_| ConvError::msg("value belongs to a closed Lua state"))?;
        let op = Op::PushRef(r.id());
        self.keep.push(r);
        Ok(self.op(op, 1))
    }

    /// Borrowed ref push (the ref must outlive `finish`).
    pub fn reference_id(&mut self, r: &LuaRef) -> Result<&mut Self, ConvError> {
        r.check(self.lua).map_err(|_| ConvError::msg("value belongs to a closed Lua state"))?;
        Ok(self.op(Op::PushRef(r.id()), 1))
    }

    pub fn table(&mut self, narr: usize, nrec: usize) -> &mut Self {
        self.op(Op::NewTable(narr.min(i32::MAX as usize) as c_int, nrec.min(i32::MAX as usize) as c_int), 1)
    }

    /// Pops key and value into the table below them.
    pub fn raw_set(&mut self) -> &mut Self {
        self.op(Op::RawSet(-3), -2)
    }

    /// Pops a value into `t[i]` of the table below it.
    pub fn raw_seti(&mut self, i: c_int) -> &mut Self {
        self.op(Op::RawSetI(-2, i), -1)
    }

    /// Stages any value.
    pub fn push<T: IntoLua>(&mut self, v: T) -> Result<&mut Self, ConvError> {
        v.into_lua(self).map(|()| self)
    }

    pub fn depth(&self) -> c_int {
        self.depth
    }

    /// Runs every staged op in one protected call. Returns `(first, count)` of pushed values.
    pub fn finish(self) -> Result<(c_int, c_int)> {
        let ops: Vec<Op> = std::iter::once(Op::CheckStack(self.max + crate::lua::RESERVE))
            .chain(self.ops.iter().copied())
            .chain(self.inputs.iter().map(|_| Op::Remove(1)))
            .collect();
        let r = self.lua.run(&self.inputs, &ops);
        drop(self.keep);
        drop(self.extra);
        r
    }
}

/// Lua type name of a slot (`nil`, `number`, ... or GMod's for userdata when cheap).
pub fn type_name(lua: &Lua, idx: c_int) -> String {
    let a = lua.api();
    // SAFETY: lua_type/lua_typename never raise.
    unsafe {
        let t = (a.lua_type)(lua.raw(), idx);
        match t {
            LUA_TNONE => "no value".to_owned(),
            t => core::ffi::CStr::from_ptr((a.lua_typename)(lua.raw(), t)).to_string_lossy().into_owned(),
        }
    }
}

pub fn mismatch(lua: &Lua, idx: c_int, expected: &str) -> ConvError {
    ConvError::new(expected, type_name(lua, idx))
}

/// Maps a Lua error raised during a read into a conversion error.
pub fn lua_err(e: Error) -> ConvError {
    match e.kind() {
        ErrorKind::Lua { message, .. } => ConvError::msg(message.clone()),
        k => ConvError::msg(k.to_string()),
    }
}

impl Lua {
    /// Runs read ops with `inputs` and copies `N` raw outputs. Results stay on the stack.
    pub fn read_ops<const N: usize>(&self, inputs: &[c_int], ops: &[Op]) -> Result<[Raw; N], ConvError> {
        let mut out = [Raw::default(); N];
        let all: Vec<Op> = std::iter::once(Op::CheckStack(8)).chain(ops.iter().copied()).collect();
        self.protect(inputs, &all, &mut out).map(|_| out).map_err(lua_err)
    }

    /// Converts the value at `idx`.
    pub fn get<T: FromLua>(&self, idx: c_int) -> Result<T, ConvError> {
        T::from_lua(self, idx)
    }

    /// Pushes one value onto the stack (protected); the caller owns the slot.
    pub fn push<T: IntoLua>(&self, v: T) -> Result<()> {
        let mut s = Stage::new(self);
        v.into_lua(&mut s)?;
        s.finish().map(|_| ())
    }

    /// Bytes of a string slot without coercion (non-raising). `None` if not a string.
    pub fn raw_bytes(&self, idx: c_int) -> Option<&[u8]> {
        let r = self.read(idx);
        // SAFETY: Lua strings are immutable while the slot is alive; callers keep it alive.
        (r.tag == LUA_TSTRING).then(|| unsafe { core::slice::from_raw_parts(r.ptr.cast::<u8>(), r.len) })
    }
}

pub use func::{LuaFunction, create_function};
pub use gmod::{Angle, Color, Entity, EntityRef, Player, PlayerRef, Vector};
pub use multi::{Chain, FromArgs, FromLuaMulti, IntoLuaMulti, Variadic};
pub use prim::{AsString, LuaBytes, Nil, SteamId64};
pub use table::LuaTable;
pub use userdata::{LuaUd, UserData, UserDataMethods};
pub use value::{LuaAny, LuaValue};
