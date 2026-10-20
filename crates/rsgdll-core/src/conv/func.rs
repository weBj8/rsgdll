//! Functions (spec §6.2): `LuaFunction` refs and Rust closures as Lua C closures.
//! The closure box lives in a full userdata upvalue whose `__gc` drops it under
//! `catch_unwind` and never touches the runtime map or Lua.

use core::ffi::{c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use rsgdll_sys::lua::*;

use super::{FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Stage, lua_err, mismatch};
use crate::error::{ConvError, Error, Result};
use crate::lua::Lua;
use crate::protected::{Op, Raw};
use crate::refs::LuaRef;
use crate::trampoline::{self, Outcome};

/// A Lua function held by registry ref.
pub struct LuaFunction(pub LuaRef);

impl FromLua for LuaFunction {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        match lua.read(idx).tag {
            LUA_TFUNCTION => LuaRef::new(lua, idx).map(LuaFunction).map_err(lua_err),
            _ => Err(mismatch(lua, idx, "function")),
        }
    }
}

impl IntoLua for LuaFunction {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.reference(self.0).map(|_| ())
    }
}

impl IntoLua for &LuaFunction {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.reference_id(&self.0).map(|_| ())
    }
}

impl LuaFunction {
    /// Calls the function; a Lua error becomes `ErrorKind::Lua` with the Lua traceback.
    pub fn call<A: IntoLuaMulti, R: FromLuaMulti>(&self, lua: &Lua, args: A) -> Result<R> {
        let g = lua.guard();
        let mut s = Stage::new(lua);
        s.reference_id(&self.0)?;
        let n = args.into_lua_multi(&mut s)?;
        s.op(Op::Call(n, LUA_MULTRET), -n - 1);
        let r = s.finish().and_then(|(first, count)| R::from_lua_multi(lua, first, count).map_err(Error::from));
        drop(g);
        r
    }

    pub fn try_clone(&self, lua: &Lua) -> Result<Self> {
        self.0.try_clone(lua).map(LuaFunction)
    }
}

type Boxed = Box<dyn Fn(&Lua) -> Outcome>;

/// Pushes a call's return values after resetting the stack to `entry` (spec §6.1).
pub fn returns<R: IntoLuaMulti>(lua: &Lua, entry: c_int, path: &str, r: R) -> Outcome {
    // SAFETY: shrinking only.
    unsafe { (lua.api().lua_settop)(lua.raw(), entry) };
    let mut s = Stage::new(lua);
    let staged = r.into_lua_multi(&mut s);
    match staged.map_err(Error::from).and_then(|_| s.finish()) {
        Ok((_, n)) => Outcome::Ok(n),
        Err(e) => Outcome::Err(trampoline::format_error(lua.module_name(), path, &e.message())),
    }
}

/// `bad argument #N to 'path' (msg)` in Lua's style (spec §8.2).
pub fn bad_argument(lua: &Lua, path: &str, n: c_int, e: &ConvError) -> Outcome {
    let detail = match (e.path.is_empty(), e.got.is_empty()) {
        (true, false) => format!("{} expected, got {}", e.expected, e.got),
        _ => e.to_string(),
    };
    Outcome::Err(format!("[{}] bad argument #{n} to '{path}' ({detail})", lua.module_name()))
}

unsafe extern "C-unwind" fn dispatch(l: *mut lua_State) -> c_int {
    unsafe {
        trampoline::call(l, &|lua| {
            let p = (lua.api().lua_touserdata)(l, lua_upvalueindex(1)).cast::<*const Boxed>();
            match p.as_ref().and_then(|b| b.as_ref()) {
                Some(f) => f(lua),
                None => Outcome::Err("rsgdll: closure already collected".into()),
            }
        })
    }
}

unsafe extern "C-unwind" fn gc(l: *mut lua_State) -> c_int {
    unsafe {
        let p = (rsgdll_sys::api().lua_touserdata)(l, 1).cast::<*mut Boxed>();
        if let Some(slot) = p.as_mut() {
            let b = core::mem::replace(slot, core::ptr::null_mut());
            if !b.is_null() {
                let r = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(b))));
                if r.is_err() {
                    rsgdll_sys::tier0::msg("rsgdll: panic in closure drop during __gc (ignored)\n");
                }
            }
        }
    }
    0
}

#[repr(C)]
struct Init {
    boxed: *mut Boxed,
    meta: c_int,
    taken: bool,
}

/// Raw protected step: userdata + pointer + metatable + C closure (no Rust allocation).
unsafe extern "C-unwind" fn make_closure(l: *mut lua_State, data: *mut c_void) {
    let a = rsgdll_sys::api();
    unsafe {
        if let Some(init) = data.cast::<Init>().as_mut() {
            let ud = (a.lua_newuserdata)(l, core::mem::size_of::<*mut Boxed>()).cast::<*mut Boxed>();
            ud.write(init.boxed);
            (a.lua_rawgeti)(l, LUA_REGISTRYINDEX, init.meta);
            (a.lua_setmetatable)(l, -2);
            init.taken = true;
            (a.lua_pushcclosure)(l, dispatch, 1);
        }
    }
}

/// Metatable with `__gc` for closure boxes, created once per state.
fn closure_meta(lua: &Lua) -> Result<c_int> {
    let rt = lua.runtime().clone();
    let cached = rt.ext::<std::cell::Cell<c_int>>("closure_meta", || std::cell::Cell::new(LUA_NOREF));
    match cached.get() {
        LUA_NOREF => {
            let g = lua.guard();
            let mut out = [Raw::default()];
            lua.protect(&[], &[Op::NewTable(0, 1), Op::PushCClosure(gc, 0), Op::SetField(-2, c"__gc".as_ptr()), Op::Ref(0)], &mut out)?;
            drop(g);
            cached.set(out[0].num as c_int);
            Ok(cached.get())
        }
        r => Ok(r),
    }
}

/// Pushes a raw closure (`f` handles the whole call) onto the stack.
pub fn push_raw_closure(lua: &Lua, f: Boxed) -> Result<()> {
    let meta = closure_meta(lua)?;
    let mut init = Init { boxed: Box::into_raw(Box::new(f)), meta, taken: false };
    let r = lua.run(&[], &[Op::CheckStack(4), Op::Raw(make_closure, crate::state::light_mut(&mut init).cast())]);
    if !init.taken {
        // SAFETY: never handed to Lua.
        drop(unsafe { Box::from_raw(init.boxed) });
    }
    r.map(|_| ())
}

/// Wraps a typed Rust closure as a Lua function (args via `FromLuaMulti`).
pub fn create_function<A, R, F>(lua: &Lua, name: &str, f: F) -> Result<LuaFunction>
where
    A: FromLuaMulti,
    R: IntoLuaMulti,
    F: Fn(&Lua, A) -> Result<R> + 'static,
{
    let path = name.to_owned();
    let boxed: Boxed = Box::new(move |lua| {
        let count = lua.top();
        match A::from_lua_multi(lua, 1, count) {
            Err(e) => bad_argument(lua, &path, 1, &e),
            Ok(args) => match f(lua, args) {
                Ok(r) => returns(lua, count, &path, r),
                Err(e) => Outcome::Err(trampoline::format_error(lua.module_name(), &path, &e.lua_message())),
            },
        }
    });
    let g = lua.guard();
    push_raw_closure(lua, boxed)?;
    let r = LuaRef::new(lua, -1).map(LuaFunction);
    drop(g);
    r
}

impl Lua {
    /// Shortcut for [`create_function`].
    pub fn create_function<A, R, F>(&self, f: F) -> Result<LuaFunction>
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(&Lua, A) -> Result<R> + 'static,
    {
        create_function(self, "<closure>", f)
    }

    /// `print(...)` through Lua's `print` (GMod console).
    pub fn print(&self, msg: impl std::fmt::Display) -> Result<()> {
        self.global::<LuaFunction>("print")?.call::<_, ()>(self, msg.to_string())
    }

    /// Reads a global (dotted paths walk tables: `"ents.FindByClass"`).
    pub fn global<T: FromLua>(&self, path: &str) -> Result<T> {
        let g = self.guard();
        let r = path.split('.').try_fold(None::<c_int>, |cur, seg| {
            let t = match cur {
                None => {
                    self.run(&[], &[Op::PushGlobals])?;
                    self.top()
                }
                Some(t) => t,
            };
            super::table::fetch_field(self, t, seg).map(Some).map_err(Error::from)
        });
        let out = r.and_then(|i| T::from_lua(self, i.unwrap_or(0)).map_err(|e| Error::from(e).context(format!("global '{path}'"))));
        drop(g);
        out
    }

    /// Sets a global (dotted path; intermediate tables are created if missing).
    pub fn set_global<T: IntoLua>(&self, path: &str, v: T) -> Result<()> {
        let g = self.guard();
        let segs: Vec<&str> = path.split('.').collect();
        let (last, parents) = segs.split_last().ok_or_else(|| crate::anyhow!("empty global path"))?;
        self.run(&[], &[Op::PushGlobals])?;
        parents.iter().try_for_each(|seg| -> Result<()> {
            let t = self.top();
            let v = super::table::fetch_field(self, t, seg)?;
            match self.read(v).tag {
                LUA_TTABLE => Ok(()),
                LUA_TNIL => {
                    // SAFETY: pop the nil.
                    unsafe { (self.api().lua_settop)(self.raw(), -2) };
                    let mut s = Stage::new(self);
                    s.table(0, 0);
                    s.finish()?;
                    let key = (*seg).to_owned();
                    let mut s = Stage::with_inputs(self, &[t, -1]);
                    s.op(Op::PushStr(key.as_ptr(), key.len()), 1).op(Op::PushValue(2), 1).op(Op::SetTable(1), -2);
                    // The new table stays on top as the next parent.
                    s.finish().map(|_| ())
                }
                _ => Err(crate::anyhow!("'{seg}' in '{path}' is not a table")),
            }
        })?;
        let mut s = Stage::with_inputs(self, &[-1]);
        s.op(Op::PushStr(last.as_ptr(), last.len()), 1);
        s.push(v)?;
        s.op(Op::SetTable(1), -2);
        let r = s.finish().map(|_| ());
        drop(g);
        r
    }
}
