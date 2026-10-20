//! Async runtime on stock LuaJIT (spec §10): handle calls, spawn_local, sleep, workers,
//! async exports, StateClosed after close. The pump is driven manually here.

mod common;

use std::time::Duration;

use rsgdll::__rt::module::{self, ModuleInfo};
use rsgdll::__rt::rt::executor;
use rsgdll::Realm;
use rsgdll::prelude::*;

#[rsgdll::function]
async fn fetch(lua: LocalLua, x: i32) -> Result<String> {
    let doubled = rsgdll::spawn_worker(move || Ok(x * 2)).await?;
    rsgdll::sleep(Duration::from_millis(5)).await;
    let s: String = lua!(&lua, return "v" .. $doubled)?;
    Ok(s)
}

#[rsgdll::function]
async fn fails() -> Result<i32> {
    Err(anyhow!("async nope"))
}

static INFO: ModuleInfo = ModuleInfo {
    name: "a",
    realms: &[Realm::Server],
    gmod: false,
};

fn open() -> (*mut rsgdll::__rt::sys::lua_State, Lua) {
    let a = common::api();
    unsafe {
        let l = (a.luaL_newstate)();
        (a.luaL_openlibs)(l);
        assert_eq!(module::open(l, &INFO), 0);
        (l, Lua::from_raw(l).unwrap())
    }
}

fn pump_until(lua: &Lua, mut done: impl FnMut() -> bool) {
    let start = std::time::Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(5), "timeout");
        executor::pump(lua);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn handle_call_from_thread_and_blocking() {
    let (l, lua) = open();
    common::exec(l, "answer = 41").unwrap();
    let h = lua.handle();
    let t = std::thread::spawn(move || {
        h.call_blocking(|lua| lua.global::<i32>("answer").map(|v| v + 1))
    });
    pump_until(&lua, || t.is_finished());
    assert_eq!(t.join().unwrap().unwrap(), 42);
    assert!(
        lua.handle().call_blocking(|_| Ok(())).is_err(),
        "main-thread call_blocking must error"
    );
    unsafe { module::close(l) };
}

#[test]
fn async_export_callback() {
    let (l, lua) = open();
    common::exec(l, "a.Fetch(4, function(err, v) got = tostring(err) .. ':' .. tostring(v) end) a.Fails(function(err) bad = err end)").unwrap();
    pump_until(&lua, || {
        common::exec(l, "return tostring(got ~= nil and bad ~= nil)").unwrap() == "true"
    });
    assert_eq!(common::exec(l, "return got").unwrap(), "nil:v8");
    assert!(
        common::exec(l, "return bad")
            .unwrap()
            .contains("[a] a.Fails: async nope")
    );
    let e = common::exec(l, "a.Fetch(1)").unwrap_err();
    assert!(e.contains("bad argument #2"), "{e}");
    unsafe { module::close(l) };
}

#[test]
fn spawn_local_and_state_closed() {
    let (l, lua) = open();
    rsgdll::spawn_local(&lua, |lua| async move {
        rsgdll::sleep(Duration::from_millis(2)).await;
        lua.lua()?.set_global("ticked", true)
    });
    pump_until(&lua, || {
        common::exec(l, "return tostring(ticked)").unwrap() == "true"
    });
    let h = lua.handle();
    // A blocked caller is released with StateClosed when the state closes.
    let waiting = std::thread::spawn(move || h.call_blocking(|_| Ok(1)));
    std::thread::sleep(Duration::from_millis(20));
    unsafe { module::close(l) };
    let r = waiting.join().unwrap();
    assert!(matches!(r.unwrap_err().kind(), ErrorKind::StateClosed));
    let h2 = lua.handle();
    assert!(h2.is_closed());
}
