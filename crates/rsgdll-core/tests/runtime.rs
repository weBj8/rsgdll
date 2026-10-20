//! Core runtime behaviour on stock LuaJIT: trampoline errors, panics, refs, protected ops.

mod common;

use rsgdll_core::protected::Op;
use rsgdll_core::trampoline::{self, Outcome};
use rsgdll_core::{ErrorKind, LuaRef};
use rsgdll_sys::lua::*;

unsafe extern "C-unwind" fn fails(l: *mut lua_State) -> i32 {
    unsafe { trampoline::call(l, &|_| Outcome::Err(trampoline::format_error("test", "test.Fail", "division by zero"))) }
}

unsafe extern "C-unwind" fn panics(l: *mut lua_State) -> i32 {
    unsafe { trampoline::call(l, &|_| panic!("boom {}", 42)) }
}

unsafe extern "C-unwind" fn ok_two(l: *mut lua_State) -> i32 {
    unsafe {
        trampoline::call(l, &|lua| match lua.run(&[], &[Op::PushNumber(1.0), Op::PushNumber(2.0)]) {
            Ok((_, n)) => Outcome::Ok(n),
            Err(e) => Outcome::Err(e.to_string()),
        })
    }
}

#[test]
fn err_becomes_lua_error() {
    let (l, _lua) = common::state();
    common::set_global(l, c"fail", fails);
    let r = common::exec(l, "local ok, e = pcall(fail) return tostring(ok) .. '|' .. e");
    assert_eq!(r.unwrap(), "false|[test] test.Fail: division by zero");
}

#[test]
fn panic_becomes_lua_error_and_state_survives() {
    let (l, _lua) = common::state();
    common::set_global(l, c"boom", panics);
    common::set_global(l, c"two", ok_two);
    let r = common::exec(l, "local ok, e = pcall(boom) return e").unwrap();
    assert!(r.starts_with("rust panic: ") && r.contains("boom 42"), "{r}");
    assert_eq!(common::exec(l, "local a, b = two() return a + b").unwrap(), "3");
}

#[test]
fn lua_error_in_protected_call_is_err_with_traceback() {
    let (l, lua) = common::state();
    common::exec(l, "function thrower() error('bad thing') end").unwrap();
    let g = lua.guard();
    let r = lua.run(&[], &[Op::PushGlobals, Op::GetField(-1, c"thrower".as_ptr()), Op::Call(0, 0)]);
    drop(g);
    match r.unwrap_err().kind() {
        ErrorKind::Lua { message, lua_traceback } => {
            assert!(message.contains("bad thing"), "{message}");
            assert!(lua_traceback.as_deref().unwrap_or("").contains("stack traceback"));
        }
        k => panic!("{k:?}"),
    }
    assert_eq!(lua.top(), 0);
}

#[test]
fn refs_are_deferred_and_flushed() {
    let (l, lua) = common::state();
    let a = common::api();
    unsafe { (a.lua_pushnumber)(l, 7.0) };
    let r = LuaRef::new(&lua, -1).unwrap();
    unsafe { (a.lua_settop)(l, 0) };
    r.push(&lua).unwrap();
    assert_eq!(unsafe { (a.lua_tonumber)(l, -1) }, 7.0);
    unsafe { (a.lua_settop)(l, 0) };
    let id = r.id();
    drop(r);
    assert_eq!(lua.runtime().unrefs.borrow().as_slice(), &[id]);
    lua.flush_unrefs().unwrap();
    assert!(lua.runtime().unrefs.borrow().is_empty());
}

#[test]
fn closed_state_refs_report_state_closed() {
    let (l, lua) = common::state();
    let a = common::api();
    unsafe { (a.lua_pushnil)(l) };
    let r = LuaRef::new(&lua, -1).unwrap();
    let (_l2, lua2) = common::state();
    assert!(matches!(r.push(&lua2).unwrap_err().kind(), ErrorKind::StateClosed));
    rsgdll_core::lua::close_state(lua);
    drop(r); // stale generation: discarded, no map access
}

#[test]
fn oom_fallback_message_is_interned() {
    let (_l, lua) = common::state();
    let g = lua.guard();
    lua.push_ref(lua.runtime().boot.oom_message);
    let raw = lua.read(-1);
    let s = unsafe { std::str::from_utf8(std::slice::from_raw_parts(raw.ptr.cast::<u8>(), raw.len)).unwrap() };
    assert_eq!(s, "[test] error (message allocation failed)");
    drop(g);
}

#[test]
fn checkstack_failure_is_err() {
    let (_l, lua) = common::state();
    let g = lua.guard();
    assert!(lua.run(&[], &[Op::CheckStack(10_000_000)]).is_err());
    drop(g);
    assert_eq!(lua.top(), 0);
}
