//! The `Lua` handle, `StackGuard` and the protected-op runner (spec §6.1, §7.3).

use core::ffi::{CStr, c_int, c_void};
use std::marker::PhantomData;
use std::rc::Rc;

use rsgdll_sys::lua::*;

use crate::error::{Error, Result};
use crate::protected::{Args, Boot, Op, Raw};
use crate::state::{self, Runtime};

/// Slots a protected op needs above the caller's top: interpreter, args, inputs.
pub const MAX_INPUTS: usize = 4;
/// Safe code keeps this many `LUA_MINSTACK` slots free outside protected ops (spec §5.1).
pub const RESERVE: c_int = 2 + MAX_INPUTS as c_int;

/// Handle to the current Lua state. `!Send + !Sync`.
pub struct Lua {
    l: *mut lua_State,
    rt: Rc<Runtime>,
    _not_send: PhantomData<*mut ()>,
}

/// Restores `lua_gettop` on drop (`lua_settop` downwards never raises).
pub struct StackGuard {
    l: *mut lua_State,
    top: c_int,
}

impl StackGuard {
    pub fn top(&self) -> c_int {
        self.top
    }
}

impl Drop for StackGuard {
    fn drop(&mut self) {
        // SAFETY: shrinking the stack only.
        unsafe {
            let a = rsgdll_sys::api();
            if (a.lua_gettop)(self.l) > self.top {
                (a.lua_settop)(self.l, self.top);
            }
        }
    }
}

/// Splits `msg\nstack traceback:\n...` into message + Lua traceback.
fn split_traceback(s: &str) -> (String, Option<String>) {
    match s.split_once("\nstack traceback:") {
        Some((m, tb)) => (m.to_owned(), Some(format!("stack traceback:{tb}"))),
        None => (s.to_owned(), None),
    }
}

/// Reads the error value at the top without allocating in Lua.
unsafe fn read_error(a: &LuaApi, l: *mut lua_State, status: c_int) -> Error {
    unsafe {
        let msg = match (a.lua_type)(l, -1) {
            LUA_TSTRING => {
                let mut len = 0usize;
                let p = (a.lua_tolstring)(l, -1, &mut len);
                String::from_utf8_lossy(core::slice::from_raw_parts(p.cast::<u8>(), len))
                    .into_owned()
            }
            t => {
                let name = CStr::from_ptr((a.lua_typename)(l, t)).to_string_lossy();
                format!("(error object is a {name} value)")
            }
        };
        let msg = if status == LUA_ERRMEM {
            format!("out of memory: {msg}")
        } else {
            msg
        };
        let (message, tb) = split_traceback(&msg);
        Error::lua(message, tb)
    }
}

impl Lua {
    /// Handle for `l` if this DLL has a live runtime for its state.
    ///
    /// # Safety
    /// `l` must be a valid `lua_State` used on its owning thread.
    pub unsafe fn from_raw(l: *mut lua_State) -> Result<Self> {
        state::get(l)
            .map(|rt| Self {
                l,
                rt,
                _not_send: PhantomData,
            })
            .ok_or_else(Error::state_closed)
    }

    pub(crate) fn with_runtime(l: *mut lua_State, rt: Rc<Runtime>) -> Self {
        Self {
            l,
            rt,
            _not_send: PhantomData,
        }
    }

    /// Opt-in raw access (spec §1). Everything done with it is the caller's responsibility.
    pub fn raw(&self) -> *mut lua_State {
        self.l
    }

    pub fn api(&self) -> &'static LuaApi {
        rsgdll_sys::api()
    }

    pub fn runtime(&self) -> &Rc<Runtime> {
        &self.rt
    }

    pub fn module_name(&self) -> &'static str {
        self.rt.module
    }

    pub fn generation(&self) -> u64 {
        self.rt.generation
    }

    /// Main thread of this state (refs and `ILuaBase` live there).
    pub fn main_thread(&self) -> *mut lua_State {
        self.rt.main
    }

    pub fn is_main_thread(&self) -> bool {
        self.l == self.rt.main
    }

    pub fn top(&self) -> c_int {
        // SAFETY: never raises.
        unsafe { (self.api().lua_gettop)(self.l) }
    }

    pub fn guard(&self) -> StackGuard {
        StackGuard {
            l: self.l,
            top: self.top(),
        }
    }

    /// Absolute stack index (pseudo-indices unchanged).
    pub fn abs(&self, idx: c_int) -> c_int {
        match idx {
            i if i > 0 || i <= LUA_REGISTRYINDEX => i,
            i => self.top() + i + 1,
        }
    }

    /// Raw non-raising view of a slot (strings are never coerced).
    pub fn read(&self, idx: c_int) -> Raw {
        // SAFETY: lua_type/tonumber/tolstring-on-strings/toboolean/touserdata never raise.
        unsafe { crate::protected::read(self.api(), self.l, idx) }
    }

    /// Runs `ops` in the fixed interpreter under `lua_pcall`. `inputs` are copied in as
    /// frame slots `1..=n`. Returns `(first, count)` of the results now on the stack.
    pub fn protect(&self, inputs: &[c_int], ops: &[Op], out: &mut [Raw]) -> Result<(c_int, c_int)> {
        assert!(
            inputs.len() <= MAX_INPUTS,
            "rsgdll: too many protected inputs"
        );
        let a = self.api();
        let abs: Vec<c_int> = inputs.iter().map(|i| self.abs(*i)).collect();
        let base = self.top();
        let (unrefs, n_unrefs) = self.rt.take_unrefs();
        let args = Args {
            ops: ops.as_ptr(),
            n_ops: ops.len(),
            out: out.as_mut_ptr(),
            n_out: out.len(),
            unrefs,
            n_unrefs,
            handler_ref: self.rt.boot.handler,
        };
        // SAFETY: rawgeti/pushlightuserdata/pushvalue use reserved slots and do not allocate.
        unsafe {
            (a.lua_rawgeti)(self.l, LUA_REGISTRYINDEX, self.rt.boot.interpreter);
            (a.lua_pushlightuserdata)(self.l, state::light(&args));
            abs.iter().for_each(|i| (a.lua_pushvalue)(self.l, *i));
            match (a.lua_pcall)(self.l, 1 + abs.len() as c_int, LUA_MULTRET, 0) {
                LUA_OK => Ok((base + 1, self.top() - base)),
                status => {
                    let e = read_error(a, self.l, status);
                    (a.lua_settop)(self.l, base);
                    Err(e)
                }
            }
        }
    }

    /// `protect` without output slots.
    pub fn run(&self, inputs: &[c_int], ops: &[Op]) -> Result<(c_int, c_int)> {
        self.protect(inputs, ops, &mut [])
    }

    /// Flushes deferred unrefs (no-op when the queue is empty).
    pub fn flush_unrefs(&self) -> Result<()> {
        let empty = self.rt.unrefs.borrow().is_empty();
        match empty {
            true => Ok(()),
            false => {
                let g = self.guard();
                let r = self.run(&[], &[]).map(|_| ());
                drop(g);
                r
            }
        }
    }

    /// Pushes a registry ref without allocating.
    pub fn push_ref(&self, r: c_int) {
        // SAFETY: rawgeti on the registry never raises.
        unsafe { (self.api().lua_rawgeti)(self.l, LUA_REGISTRYINDEX, r) }
    }
}

/// Bootstraps a state for this DLL (interpreter + handler + OOM message refs) and
/// registers its runtime. `l` must be the main thread.
///
/// # Safety
/// `l` must be a valid main-thread `lua_State` owned by the calling thread.
pub unsafe fn open_state(l: *mut lua_State, module: &'static str) -> Result<Lua> {
    let a = rsgdll_sys::api();
    let oom: &'static str =
        Box::leak(format!("[{module}] error (message allocation failed)").into_boxed_str());
    let mut boot = Boot {
        oom_text: (oom.as_ptr() as usize, oom.len()),
        ..Boot::default()
    };
    // SAFETY: bootstrap only creates closures/strings and refs them; errors are caught by cpcall.
    let status = unsafe {
        (a.lua_cpcall)(
            l,
            crate::protected::bootstrap,
            (&mut boot as *mut Boot).cast::<c_void>(),
        )
    };
    match status {
        LUA_OK => Ok(Lua::with_runtime(l, state::insert(l, module, boot))),
        s => {
            // SAFETY: cpcall left the error object on top.
            let e = unsafe { read_error(a, l, s) };
            unsafe { (a.lua_settop)(l, -2) };
            Err(e)
        }
    }
}

/// Unrefs the bootstrap refs and removes the runtime entry (spec §8.1 close order, last step).
pub fn close_state(lua: Lua) {
    let b = &lua.rt.boot;
    let ops = [
        Op::Unref(b.handler),
        Op::Unref(b.oom_message),
        Op::Unref(b.interpreter),
    ];
    let g = lua.guard();
    // shortcut: unref failure (OOM) only leaks three registry slots of a closing state.
    let _ = lua.run(&[], &ops);
    drop(g);
    state::remove(lua.main_thread());
}
