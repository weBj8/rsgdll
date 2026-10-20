//! Fixed protected operations (spec §7.3).
//!
//! One interpreter callback executes a batch of `Copy` [`Op`]s inside `lua_pcall`. It owns
//! nothing with `Drop`, never calls user code, never allocates in Rust and cannot panic, so
//! a Lua error (longjmp or foreign exception) through it skips nothing. All raising or
//! allocating Lua API calls made by the safe layer go through here.
#![deny(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use core::ffi::{c_char, c_int, c_void};

use rsgdll_sys::lua::*;

/// Max deferred unrefs flushed per op; leftovers go to the next op (spec §6.2).
pub const MAX_UNREFS: usize = 16;

/// One stack instruction. Indices are frame-relative: inputs are `1..=inputs`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum Op {
    CheckStack(c_int),
    PushNil,
    PushBool(bool),
    PushNumber(f64),
    PushStr(*const u8, usize),
    PushLight(*mut c_void),
    PushCClosure(CFunc, c_int),
    PushValue(c_int),
    /// `lua_rawgeti(REGISTRY, ref)`
    PushRef(c_int),
    PushGlobals,
    Pop(c_int),
    Insert(c_int),
    Remove(c_int),
    Replace(c_int),
    NewTable(c_int, c_int),
    GetTable(c_int),
    SetTable(c_int),
    RawGet(c_int),
    RawSet(c_int),
    GetField(c_int, *const c_char),
    SetField(c_int, *const c_char),
    RawGetI(c_int, c_int),
    RawSetI(c_int, c_int),
    /// Calls with a traceback handler; a Lua error is re-raised with the traceback attached.
    Call(c_int, c_int),
    /// Pops the top into a registry ref; id written to `out[slot].num`.
    Ref(usize),
    Unref(c_int),
    /// Raw copy of the value at idx into `out[slot]` (strings without coercion).
    Read(c_int, usize),
    /// `lua_tolstring` (coerces numbers in place, allocates).
    ToStr(c_int, usize),
    /// `lua_objlen` into `out[slot].len`.
    Len(c_int, usize),
    /// `lua_next(idx)`; `out[slot].num` = 1 if a pair was pushed.
    Next(c_int, usize),
    /// `lua_newuserdata(size)`; pointer into `out[slot].ptr`.
    NewUserdata(usize, usize),
    SetMetatable(c_int),
    /// Pushes the metatable (or nothing); `out[slot].num` = 1 if found.
    GetMetatable(c_int, usize),
    RawEqual(c_int, c_int, usize),
    Concat(c_int),
    /// `lua_xmove(from, to, n)` between threads of one state.
    XMove(*mut lua_State, *mut lua_State, c_int),
    /// `lua_checkstack` on another thread (main stack before `ILuaBase` pushes, spec §6.1a).
    CheckStackOf(*mut lua_State, c_int),
    /// `luaL_loadbuffer(ptr, len, chunkname)`; a syntax error is raised as a Lua error.
    Load(*const u8, usize, *const c_char),
    /// Arbitrary raw callback on this frame; must follow the same rules as this module.
    Raw(
        unsafe extern "C-unwind" fn(*mut lua_State, *mut c_void),
        *mut c_void,
    ),
}

/// Raw value copied out of a stack slot. Pointers stay valid while the slot is alive.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Raw {
    pub tag: c_int,
    pub num: f64,
    pub ptr: *const c_void,
    pub len: usize,
}

impl Default for Raw {
    fn default() -> Self {
        Self {
            tag: LUA_TNONE,
            num: 0.0,
            ptr: core::ptr::null(),
            len: 0,
        }
    }
}

/// Arguments passed as light userdata.
#[repr(C)]
pub struct Args {
    pub ops: *const Op,
    pub n_ops: usize,
    pub out: *mut Raw,
    pub n_out: usize,
    pub unrefs: [c_int; MAX_UNREFS],
    pub n_unrefs: usize,
    pub handler_ref: c_int,
}

unsafe fn put(args: &Args, slot: usize, v: Raw) {
    if slot < args.n_out && !args.out.is_null() {
        // SAFETY: slot bounds-checked against n_out.
        unsafe { *args.out.add(slot) = v };
    }
}

unsafe fn step(a: &LuaApi, l: *mut lua_State, args: &Args, op: Op) {
    unsafe {
        match op {
            Op::CheckStack(n) => {
                if (a.lua_checkstack)(l, n) == 0 {
                    (a.lua_pushstring)(l, c"stack overflow (rsgdll)".as_ptr());
                    (a.lua_error)(l);
                }
            }
            Op::PushNil => (a.lua_pushnil)(l),
            Op::PushBool(b) => (a.lua_pushboolean)(l, c_int::from(b)),
            Op::PushNumber(n) => (a.lua_pushnumber)(l, n),
            Op::PushStr(p, n) => (a.lua_pushlstring)(l, p.cast(), n),
            Op::PushLight(p) => (a.lua_pushlightuserdata)(l, p),
            Op::PushCClosure(f, n) => (a.lua_pushcclosure)(l, f, n),
            Op::PushValue(i) => (a.lua_pushvalue)(l, i),
            Op::PushRef(r) => (a.lua_rawgeti)(l, LUA_REGISTRYINDEX, r),
            Op::PushGlobals => (a.lua_pushvalue)(l, LUA_GLOBALSINDEX),
            Op::Pop(n) => (a.lua_settop)(l, -n - 1),
            Op::Insert(i) => (a.lua_insert)(l, i),
            Op::Remove(i) => (a.lua_remove)(l, i),
            Op::Replace(i) => (a.lua_replace)(l, i),
            Op::NewTable(n, r) => (a.lua_createtable)(l, n, r),
            Op::GetTable(i) => (a.lua_gettable)(l, i),
            Op::SetTable(i) => (a.lua_settable)(l, i),
            Op::RawGet(i) => (a.lua_rawget)(l, i),
            Op::RawSet(i) => (a.lua_rawset)(l, i),
            Op::GetField(i, k) => (a.lua_getfield)(l, i, k),
            Op::SetField(i, k) => (a.lua_setfield)(l, i, k),
            Op::RawGetI(i, n) => (a.lua_rawgeti)(l, i, n),
            Op::RawSetI(i, n) => (a.lua_rawseti)(l, i, n),
            Op::Call(nargs, nres) => {
                let func = (a.lua_gettop)(l) - nargs;
                (a.lua_rawgeti)(l, LUA_REGISTRYINDEX, args.handler_ref);
                (a.lua_insert)(l, func);
                let status = (a.lua_pcall)(l, nargs, nres, func);
                (a.lua_remove)(l, func);
                if status != LUA_OK {
                    (a.lua_error)(l);
                }
            }
            Op::Ref(slot) => {
                let id = (a.luaL_ref)(l, LUA_REGISTRYINDEX);
                put(
                    args,
                    slot,
                    Raw {
                        num: f64::from(id),
                        ..Raw::default()
                    },
                );
            }
            Op::Unref(r) => (a.luaL_unref)(l, LUA_REGISTRYINDEX, r),
            Op::Read(i, slot) => put(args, slot, read(a, l, i)),
            Op::ToStr(i, slot) => {
                let mut len = 0usize;
                let p = (a.lua_tolstring)(l, i, &mut len);
                put(
                    args,
                    slot,
                    Raw {
                        tag: (a.lua_type)(l, i),
                        ptr: p.cast(),
                        len,
                        num: 0.0,
                    },
                );
            }
            Op::Len(i, slot) => put(
                args,
                slot,
                Raw {
                    len: (a.lua_objlen)(l, i),
                    ..Raw::default()
                },
            ),
            Op::Next(i, slot) => {
                let more = (a.lua_next)(l, i);
                put(
                    args,
                    slot,
                    Raw {
                        num: f64::from(more),
                        ..Raw::default()
                    },
                );
            }
            Op::NewUserdata(size, slot) => {
                let p = (a.lua_newuserdata)(l, size);
                put(
                    args,
                    slot,
                    Raw {
                        ptr: p,
                        ..Raw::default()
                    },
                );
            }
            Op::SetMetatable(i) => {
                (a.lua_setmetatable)(l, i);
            }
            Op::GetMetatable(i, slot) => {
                let found = (a.lua_getmetatable)(l, i);
                put(
                    args,
                    slot,
                    Raw {
                        num: f64::from(found),
                        ..Raw::default()
                    },
                );
            }
            Op::RawEqual(x, y, slot) => {
                let eq = (a.lua_rawequal)(l, x, y);
                put(
                    args,
                    slot,
                    Raw {
                        num: f64::from(eq),
                        ..Raw::default()
                    },
                );
            }
            Op::Concat(n) => (a.lua_concat)(l, n),
            Op::XMove(from, to, n) => (a.lua_xmove)(from, to, n),
            Op::CheckStackOf(t, n) => {
                if (a.lua_checkstack)(t, n) == 0 {
                    (a.lua_pushstring)(l, c"stack overflow (rsgdll, main thread)".as_ptr());
                    (a.lua_error)(l);
                }
            }
            Op::Load(p, n, name) => {
                if (a.luaL_loadbuffer)(l, p.cast(), n, name) != LUA_OK {
                    (a.lua_error)(l);
                }
            }
            Op::Raw(f, data) => f(l, data),
        }
    }
}

/// Copies a raw view of a slot. Never coerces, never allocates.
///
/// # Safety
/// Called by Lua (or `read` by the safe layer) with a live `lua_State` on its owning thread.
pub unsafe fn read(a: &LuaApi, l: *mut lua_State, i: c_int) -> Raw {
    unsafe {
        let tag = (a.lua_type)(l, i);
        match tag {
            LUA_TSTRING => {
                let mut len = 0usize;
                let p = (a.lua_tolstring)(l, i, &mut len);
                Raw {
                    tag,
                    ptr: p.cast(),
                    len,
                    num: 0.0,
                }
            }
            LUA_TNUMBER => Raw {
                tag,
                num: (a.lua_tonumber)(l, i),
                ..Raw::default()
            },
            LUA_TBOOLEAN => Raw {
                tag,
                num: f64::from((a.lua_toboolean)(l, i)),
                ..Raw::default()
            },
            LUA_TUSERDATA | LUA_TLIGHTUSERDATA => Raw {
                tag,
                ptr: (a.lua_touserdata)(l, i),
                ..Raw::default()
            },
            _ => Raw {
                tag,
                ptr: (a.lua_topointer)(l, i),
                ..Raw::default()
            },
        }
    }
}

/// The interpreter. Stack on entry: `[args lightuserdata, inputs...]`; returns everything
/// left on its frame after the ops ran.
///
/// # Safety
/// Called by Lua (or `read` by the safe layer) with a live `lua_State` on its owning thread.
pub unsafe extern "C-unwind" fn interpreter(l: *mut lua_State) -> c_int {
    let a = rsgdll_sys::api();
    unsafe {
        let args = (a.lua_touserdata)(l, 1).cast::<Args>().cast_const();
        (a.lua_remove)(l, 1);
        if let Some(args) = args.as_ref() {
            let unrefs = args.unrefs.iter().take(args.n_unrefs.min(MAX_UNREFS));
            unrefs.for_each(|r| (a.luaL_unref)(l, LUA_REGISTRYINDEX, *r));
            if !args.ops.is_null() {
                core::slice::from_raw_parts(args.ops, args.n_ops)
                    .iter()
                    .for_each(|op| step(a, l, args, *op));
            }
        }
        (a.lua_gettop)(l)
    }
}

/// Message handler for [`Op::Call`]: appends `luaL_traceback` to string messages.
///
/// # Safety
/// Called by Lua (or `read` by the safe layer) with a live `lua_State` on its owning thread.
pub unsafe extern "C-unwind" fn traceback_handler(l: *mut lua_State) -> c_int {
    let a = rsgdll_sys::api();
    unsafe {
        if (a.lua_type)(l, 1) == LUA_TSTRING {
            let msg = (a.lua_tolstring)(l, 1, core::ptr::null_mut());
            (a.luaL_traceback)(l, l, msg, 1);
        }
    }
    1
}

/// Output of the one-time bootstrap run with `lua_cpcall` at open.
#[repr(C)]
#[derive(Default)]
pub struct Boot {
    pub interpreter: c_int,
    pub handler: c_int,
    pub oom_message: c_int,
    pub oom_text: (usize, usize),
}

/// Creates the interpreter/handler closures and the OOM fallback message as registry refs.
///
/// # Safety
/// Called by Lua (or `read` by the safe layer) with a live `lua_State` on its owning thread.
pub unsafe extern "C-unwind" fn bootstrap(l: *mut lua_State) -> c_int {
    let a = rsgdll_sys::api();
    unsafe {
        if let Some(boot) = (a.lua_touserdata)(l, 1).cast::<Boot>().as_mut() {
            (a.lua_pushcclosure)(l, interpreter, 0);
            boot.interpreter = (a.luaL_ref)(l, LUA_REGISTRYINDEX);
            (a.lua_pushcclosure)(l, traceback_handler, 0);
            boot.handler = (a.luaL_ref)(l, LUA_REGISTRYINDEX);
            (a.lua_pushlstring)(l, boot.oom_text.0 as *const c_char, boot.oom_text.1);
            boot.oom_message = (a.luaL_ref)(l, LUA_REGISTRYINDEX);
        }
    }
    0
}
