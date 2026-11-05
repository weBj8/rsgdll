//! Native debug hooks and live-frame inspection (spec §5.1a).
//!
//! # Safety contract (hook callbacks)
//!
//! LuaJIT calls a `lua_Hook` synchronously on the Lua owner thread while the hooked Lua frame
//! is paused. From a [`set_hook`] callback the following is supported:
//!
//! - [`Lua::frame`] and every [`Frame`] method, and any safe-layer call that goes through
//!   [`Lua::protect`] (conversion, globals, `print`, function calls). LuaJIT guarantees
//!   `LUA_MINSTACK` free slots on hook entry (`callhook` checks `1 + LUA_MINSTACK`), which
//!   covers the protected-op setup (`RESERVE`); everything else runs under `lua_pcall`.
//! - Frame access calls the debug API directly, not inside the protected interpreter: there
//!   the innermost frame would resolve against the interpreter's pc (LuaJIT encodes the top
//!   frame with size 0), losing line and local names. Those calls never raise: `getstack`
//!   and `getinfo("nSl")` push nothing, and `getlocal`/`getinfo("f")`+`getupvalue` push at
//!   most two values into the `RESERVE` slots safe code keeps free (spec §5.1), so the stack
//!   never has to grow. Value conversion then runs protected as usual.
//! - Errors raised by those operations (Lua errors, OOM) are caught by that `lua_pcall` and
//!   returned as `Err`: they never unwind through the hook into the paused Lua code.
//! - Panics are caught at the hook boundary and printed; the paused code then resumes.
//! - LuaJIT disables hooks while a hook runs, so Lua code called from the callback is not
//!   hooked and the callback is never re-entered.
//!
//! Not supported: raising a Lua error into the hooked code (use the returned `Result`
//! instead), `lua_yield` from the hook, and touching the state from another thread (the hook
//! is a no-op on threads that have no runtime for the state).
//!
//! One hook slot exists per Lua state (LuaJIT keeps it in `global_State`). [`set_hook`]
//! refuses to replace a hook it did not install; [`close_state`](crate::lua::close_state)
//! removes the module's hook before the runtime goes away.

use core::ffi::{CStr, c_char, c_int};
use std::cell::RefCell;
use std::rc::Rc;

use rsgdll_sys::LuaDebugApi;
use rsgdll_sys::lua::*;

pub use rsgdll_sys::lua::{
    LUA_HOOKCALL, LUA_HOOKCOUNT, LUA_HOOKLINE, LUA_HOOKRET, LUA_HOOKTAILRET, LUA_MASKCALL,
    LUA_MASKCOUNT, LUA_MASKLINE, LUA_MASKRET,
};

use crate::conv::{FromLua, IntoLua};
use crate::error::{Error, Result};
use crate::lua::Lua;

/// Event delivered to a hook callback (`lua_Debug.event` / `currentline` at entry).
#[derive(Clone, Copy, Debug)]
pub struct HookEvent {
    /// `LUA_HOOKCALL`, `LUA_HOOKRET`, `LUA_HOOKLINE` or `LUA_HOOKCOUNT`.
    pub event: c_int,
    /// Current line for `LUA_HOOKLINE`, otherwise as reported by LuaJIT (often -1).
    pub line: c_int,
}

type HookFn = dyn Fn(&Lua, HookEvent);
type Slot = RefCell<Option<Rc<HookFn>>>;
const SLOT: &str = "rsgdll.debug.hook";

fn dbg() -> Result<&'static LuaDebugApi> {
    rsgdll_sys::debug_api().map_err(Error::msg)
}

fn slot(lua: &Lua) -> Rc<Slot> {
    lua.runtime().ext(SLOT, || Slot::new(None))
}

fn ours(h: lua_Hook) -> bool {
    h.is_some_and(|h| core::ptr::fn_addr_eq(h, hook_entry as unsafe extern "C-unwind" fn(_, _)))
}

/// Installs `f` as the state's debug hook (`lua_sethook(mask, count)`), replacing this
/// module's previous callback. Fails if another module or library installed a hook.
pub fn set_hook(
    lua: &Lua,
    mask: c_int,
    count: c_int,
    f: impl Fn(&Lua, HookEvent) + 'static,
) -> Result<()> {
    let d = dbg()?;
    // SAFETY: gethook/sethook only read/write the hook fields of global_State; never raise.
    unsafe {
        let cur = (d.lua_gethook)(lua.raw());
        if cur.is_some() && !ours(cur) {
            return Err(Error::msg("rsgdll: a foreign Lua debug hook is installed"));
        }
        slot(lua).replace(Some(Rc::new(f)));
        (d.lua_sethook)(lua.raw(), Some(hook_entry), mask, count);
    }
    Ok(())
}

/// Removes this module's hook (no-op if none, or if the installed hook is not ours).
pub fn remove_hook(lua: &Lua) -> Result<()> {
    let d = dbg()?;
    let had = lua
        .runtime()
        .ext
        .borrow()
        .get(SLOT)
        .and_then(|s| s.clone().downcast::<Slot>().ok())
        .and_then(|s| s.take());
    // SAFETY: as in `set_hook`.
    unsafe {
        if had.is_some() && ours((d.lua_gethook)(lua.raw())) {
            (d.lua_sethook)(lua.raw(), None, 0, 0);
        }
    }
    Ok(())
}

unsafe extern "C-unwind" fn hook_entry(l: *mut lua_State, ar: *mut lua_Debug) {
    // SAFETY: LuaJIT passes a valid record for the duration of the call.
    let (event, line) = unsafe { ar.as_ref() }.map_or((-1, -1), |a| (a.event, a.currentline));
    let r = crate::trampoline::catch(|| {
        // SAFETY: called by LuaJIT on the thread running `l`; a missing runtime (closed state,
        // foreign thread) makes this a no-op.
        let Ok(lua) = (unsafe { Lua::from_raw(l) }) else {
            return;
        };
        let f = slot(&lua).borrow().clone();
        if let Some(f) = f {
            let g = lua.guard();
            f(&lua, HookEvent { event, line });
            drop(g);
        }
    });
    if let Err(msg) = r {
        // SAFETY: as above.
        if let Ok(lua) = unsafe { Lua::from_raw(l) } {
            let g = lua.guard();
            let _ = lua.print(format!(
                "[{}] debug hook panicked: {msg}",
                lua.module_name()
            ));
            drop(g);
        }
    }
}

/// Live activation record from `lua_getstack`. Valid while that frame stays on the stack.
pub struct Frame<'l> {
    lua: &'l Lua,
    ar: lua_Debug,
}

/// Owned copy of `lua_getinfo(L, "nSl", ar)`.
#[derive(Clone, Debug, Default)]
pub struct FrameInfo {
    pub name: Option<String>,
    pub namewhat: String,
    /// `"Lua"`, `"C"`, `"main"` or `"tail"`.
    pub what: String,
    pub source: String,
    pub short_src: String,
    pub currentline: c_int,
    pub linedefined: c_int,
    pub lastlinedefined: c_int,
    pub nups: c_int,
}

fn owned(p: *const c_char) -> Option<String> {
    // SAFETY: LuaJIT returns NUL-terminated strings that live as long as the frame/prototype.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

impl Lua {
    /// `lua_getstack(level)` relative to the running code: in a hook, level 0 is the hooked
    /// function; in an exported function, level 0 is that function. `None` past the top.
    pub fn frame(&self, level: c_int) -> Result<Option<Frame<'_>>> {
        let d = dbg()?;
        let mut ar = lua_Debug::default();
        // SAFETY: lua_getstack only walks frames and fills `ar`; never raises or pushes.
        let ok = unsafe { (d.lua_getstack)(self.raw(), level, &mut ar) } != 0;
        Ok(ok.then_some(Frame { lua: self, ar }))
    }
}

impl Frame<'_> {
    /// Function name, source and line information of this frame.
    pub fn info(&self) -> Result<FrameInfo> {
        let d = dbg()?;
        let mut ar = self.ar;
        // SAFETY: `ar` came from lua_getstack on this thread; "nSl" neither pushes nor raises.
        if unsafe { (d.lua_getinfo)(self.lua.raw(), c"nSl".as_ptr(), &mut ar) } == 0 {
            return Err(Error::msg(
                "rsgdll: lua_getinfo failed (frame no longer live)",
            ));
        }
        let src = ar
            .short_src
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect::<Vec<_>>();
        Ok(FrameInfo {
            name: owned(ar.name),
            namewhat: owned(ar.namewhat).unwrap_or_default(),
            what: owned(ar.what).unwrap_or_default(),
            source: owned(ar.source).unwrap_or_default(),
            short_src: String::from_utf8_lossy(&src).into_owned(),
            currentline: ar.currentline,
            linedefined: ar.linedefined,
            lastlinedefined: ar.lastlinedefined,
            nups: ar.nups,
        })
    }

    /// Local `n` (1-based, `lua_getlocal`): name and a converted copy of its value.
    /// Temporaries are reported as `"(*temporary)"`. `None` past the last active local.
    pub fn local<T: FromLua>(&self, n: c_int) -> Result<Option<(String, T)>> {
        let d = dbg()?;
        let g = self.lua.guard();
        // SAFETY: pushes at most one value into the reserved free slots (see module docs).
        let name = unsafe { (d.lua_getlocal)(self.lua.raw(), &self.ar, n) };
        let r = match owned(name) {
            Some(name) => Some((name, T::from_lua(self.lua, -1).map_err(Error::from)?)),
            None => None,
        };
        drop(g);
        Ok(r)
    }

    /// All active locals of this frame, in declaration order.
    pub fn locals<T: FromLua>(&self) -> Result<Vec<(String, T)>> {
        (1..).map_while(|n| self.local(n).transpose()).collect()
    }

    /// Sets local `n` to `v` (`lua_setlocal`). Returns the local's name, `None` if absent.
    pub fn set_local<T: IntoLua>(&self, n: c_int, v: T) -> Result<Option<String>> {
        let d = dbg()?;
        let g = self.lua.guard();
        self.lua.push(v)?;
        // SAFETY: pops the pushed value (or leaves it for the guard); never raises.
        let name = unsafe { (d.lua_setlocal)(self.lua.raw(), &self.ar, n) };
        drop(g);
        Ok(owned(name))
    }

    /// Upvalue `n` (1-based) of this frame's function: name (empty for C functions) and value.
    pub fn upvalue<T: FromLua>(&self, n: c_int) -> Result<Option<(String, T)>> {
        let d = dbg()?;
        let g = self.lua.guard();
        let mut ar = self.ar;
        // SAFETY: getinfo "f" pushes the function, getupvalue at most one value: two of the
        // reserved free slots (see module docs).
        let name = unsafe {
            match (d.lua_getinfo)(self.lua.raw(), c"f".as_ptr(), &mut ar) {
                0 => core::ptr::null(),
                _ => (d.lua_getupvalue)(self.lua.raw(), -1, n),
            }
        };
        let r = match owned(name) {
            Some(name) => Some((name, T::from_lua(self.lua, -1).map_err(Error::from)?)),
            None => None,
        };
        drop(g);
        Ok(r)
    }
}
