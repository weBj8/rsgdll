//! Native debug hooks and frame inspection on stock LuaJIT (spec §5.1a).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use rsgdll_core::debug::{self, LUA_HOOKLINE, LUA_MASKCALL, LUA_MASKLINE};
use rsgdll_core::{LuaFunction, LuaValue};
use rsgdll_sys::lua::*;

const FAILING: &str = "local function f()\nlocal secret = 42\nlocal t = nil\nreturn t.x\nend\n\
                       local ok, err = pcall(f) return tostring(ok)";

#[test]
fn line_hook_sees_locals_before_unwind() {
    let (l, lua) = common::state();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    debug::set_hook(&lua, LUA_MASKLINE, 0, move |lua, ev| {
        assert_eq!(ev.event, LUA_HOOKLINE);
        let f = lua.frame(0).unwrap().expect("hooked frame");
        let info = f.info().unwrap();
        let locals: Vec<(String, LuaValue)> = f.locals().unwrap();
        s.borrow_mut()
            .push((info.short_src, info.currentline, locals));
    })
    .unwrap();
    assert_eq!(common::exec(l, FAILING), Ok("false".into()));
    debug::remove_hook(&lua).unwrap();
    let seen = seen.borrow();
    let hit = seen.iter().rev().find(|(_, _, ls)| {
        ls.iter()
            .any(|(n, v)| n == "secret" && v.as_number() == Some(42.0))
    });
    let (src, line, _) = hit.expect("secret=42 observed in a live frame");
    assert_eq!((src.as_str(), *line), ("test", 4));
}

#[test]
fn errors_inside_hook_are_returned_not_raised() {
    let (l, lua) = common::state();
    let errs = Rc::new(RefCell::new(0));
    let e = errs.clone();
    debug::set_hook(&lua, LUA_MASKLINE, 0, move |lua, _| {
        let error: LuaFunction = lua.global("error").unwrap();
        assert!(error.call::<_, ()>(lua, "from hook").is_err());
        // Out-of-range level / local are reported, not raised.
        assert!(lua.frame(1000).unwrap().is_none());
        let f = lua.frame(0).unwrap().unwrap();
        assert!(f.local::<LuaValue>(200).unwrap().is_none());
        *e.borrow_mut() += 1;
    })
    .unwrap();
    assert_eq!(
        common::exec(l, "local a = 1\nlocal b = a + 1\nreturn b"),
        Ok("2".into())
    );
    debug::remove_hook(&lua).unwrap();
    assert!(*errs.borrow() >= 2);
}

#[test]
fn panic_in_hook_does_not_unwind_into_lua() {
    let (l, lua) = common::state();
    debug::set_hook(&lua, LUA_MASKCALL, 0, |_, _| panic!("hook boom")).unwrap();
    assert_eq!(
        common::exec(l, "return (function() return 7 end)()"),
        Ok("7".into())
    );
    debug::remove_hook(&lua).unwrap();
}

#[test]
fn set_local_and_upvalue() {
    let (l, lua) = common::state();
    debug::set_hook(&lua, LUA_MASKLINE, 0, |lua, ev| {
        let f = lua.frame(0).unwrap().unwrap();
        if ev.line == 3 && f.info().unwrap().what == "Lua" {
            let (n, v) = f.upvalue::<LuaValue>(1).unwrap().expect("upvalue");
            assert_eq!((n.as_str(), v.as_number()), ("up", Some(5.0)));
            assert_eq!(f.set_local(1, 99.0).unwrap().as_deref(), Some("v"));
        }
    })
    .unwrap();
    let src = "local up = 5\nlocal function g() local v = up\nreturn v end\nreturn g()";
    assert_eq!(common::exec(l, src), Ok("99".into()));
    debug::remove_hook(&lua).unwrap();
}

#[test]
fn foreign_hook_refused_and_close_removes_ours() {
    unsafe extern "C-unwind" fn foreign(_: *mut lua_State, _: *mut lua_Debug) {}
    let (l, lua) = common::state();
    let d = rsgdll_sys::debug_api().unwrap();
    unsafe { (d.lua_sethook)(l, Some(foreign), LUA_MASKCALL, 0) };
    assert!(debug::set_hook(&lua, LUA_MASKCALL, 0, |_, _| {}).is_err());
    unsafe { (d.lua_sethook)(l, None, 0, 0) };
    debug::set_hook(&lua, LUA_MASKCALL, 0, |_, _| {}).unwrap();
    assert!(unsafe { (d.lua_gethook)(l) }.is_some());
    rsgdll_core::lua::close_state(lua);
    assert!(unsafe { (d.lua_gethook)(l) }.is_none());
    assert_eq!(common::exec(l, "return 1"), Ok("1".into()));
}

#[test]
fn pause_loop_served_from_channel_in_coroutine() {
    use std::sync::mpsc;
    enum Cmd {
        Read(&'static str),
        Continue,
    }
    let (l, lua) = common::state();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (out_tx, out_rx) = mpsc::channel::<Option<f64>>();
    // Worker only talks over channels; it never touches Lua.
    let worker = std::thread::spawn(move || {
        let r = ["x", "up", "missing"].map(|n| {
            cmd_tx.send(Cmd::Read(n)).unwrap();
            out_rx.recv().unwrap()
        });
        cmd_tx.send(Cmd::Continue).unwrap();
        r
    });
    let a = common::api();
    debug::set_hook(&lua, LUA_MASKLINE, 0, move |lua, ev| {
        let f = lua.frame(0).unwrap().unwrap();
        if ev.line != 4 || f.info().unwrap().what != "Lua" {
            return;
        }
        let top = unsafe { (a.lua_gettop)(lua.raw()) };
        // Shadowed binding: the innermost `x` is the later local slot.
        let ls: Vec<(String, LuaValue)> = f.locals().unwrap();
        let xs: Vec<_> = ls
            .iter()
            .filter(|(n, _)| n == "x")
            .map(|(_, v)| v.as_number())
            .collect();
        assert_eq!(xs, [Some(1.0), Some(2.0)]);
        while let Ok(Cmd::Read(name)) = cmd_rx.recv() {
            // Protected helper frames between reads must not disturb the stopped activation.
            let tostring: LuaFunction = lua.global("tostring").unwrap();
            let _: String = tostring.call(lua, 1.0).unwrap();
            let f = lua.frame(0).unwrap().unwrap();
            let v = match name {
                "up" => f.upvalue::<LuaValue>(1).unwrap().map(|(_, v)| v),
                n => f
                    .locals::<LuaValue>()
                    .unwrap()
                    .into_iter()
                    .rev()
                    .find(|(k, _)| k == n)
                    .map(|(_, v)| v),
            };
            out_tx.send(v.and_then(|v| v.as_number())).unwrap();
        }
        assert_eq!(unsafe { (a.lua_gettop)(lua.raw()) }, top);
    })
    .unwrap();
    let src = "local up = 9\nlocal co = coroutine.create(function() local x = 1\ndo local x = 2\nlocal y = x + up\nend end)\nassert(coroutine.resume(co)) return 'ok'";
    assert_eq!(common::exec(l, src), Ok("ok".into()));
    assert_eq!(worker.join().unwrap(), [Some(2.0), Some(9.0), None]);
    debug::remove_hook(&lua).unwrap();
}

#[test]
fn failures_keep_stack_top() {
    let (l, lua) = common::state();
    let a = common::api();
    debug::set_hook(&lua, LUA_MASKCALL, 0, move |lua, _| {
        let top = unsafe { (a.lua_gettop)(lua.raw()) };
        let load: LuaFunction = lua.global("loadstring").unwrap();
        let _ = load.call::<_, LuaValue>(lua, "this is not lua");
        let error: LuaFunction = lua.global("error").unwrap();
        let _ = error.call::<_, ()>(lua, LuaValue::Nil);
        let f = lua.frame(0).unwrap().unwrap();
        let _ = f.local::<String>(1); // conversion failure on non-string locals
        let _ = f.set_local(500, 1.0);
        assert!(lua.frame(-1).unwrap().is_none());
        assert_eq!(unsafe { (a.lua_gettop)(lua.raw()) }, top);
    })
    .unwrap();
    assert_eq!(
        common::exec(l, "local function f(a) return a end return f(3)"),
        Ok("3".into())
    );
    debug::remove_hook(&lua).unwrap();
}

#[test]
fn reattach_to_replacement_state_rejects_old_handles() {
    let (l1, lua1) = common::state();
    let old: LuaFunction = lua1.global("print").unwrap();
    debug::set_hook(&lua1, LUA_MASKCALL, 0, |_, _| {}).unwrap();
    rsgdll_core::lua::close_state(lua1);
    let a = common::api();
    unsafe { (a.lua_close)(l1) };
    let (l2, lua2) = common::state();
    let hits = Rc::new(RefCell::new(0));
    let h = hits.clone();
    debug::set_hook(&lua2, LUA_MASKCALL, 0, move |_, _| *h.borrow_mut() += 1).unwrap();
    assert!(old.call::<_, ()>(&lua2, ()).is_err());
    assert_eq!(
        common::exec(l2, "return (function() return 1 end)()"),
        Ok("1".into())
    );
    assert!(*hits.borrow() > 0);
    rsgdll_core::lua::close_state(lua2);
}
