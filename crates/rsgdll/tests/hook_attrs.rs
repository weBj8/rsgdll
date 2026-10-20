//! `#[rsgdll::hook]` / `#[rsgdll::timer]` registered at open, removed at close (spec §10.3/10.4).

mod common;

use rsgdll::prelude::*;
use rsgdll::Realm;
use rsgdll::__rt::module::{self, ModuleInfo};

#[rsgdll::hook(PlayerSay)]
fn on_say(_ply: Option<PlayerRef<'_>>, text: String) -> Result<Option<String>> {
    Ok(text.starts_with("!ping").then(|| "pong".to_owned()))
}

#[rsgdll::hook("MyAddon.Event")]
fn on_event(lua: &Lua, a: i32) -> Result<()> {
    lua.set_global("event_a", a)
}

#[rsgdll::timer(every = "1s")]
fn tick(lua: &Lua) -> Result<()> {
    lua.set_global("ticked", true)
}

static INFO: ModuleInfo = ModuleInfo { name: "h", realms: &[Realm::Server], gmod: false };

const MOCK: &str = r#"
hook = { t = {} }
function hook.Add(ev, id, f) hook.t[ev] = hook.t[ev] or {} hook.t[ev][id] = f end
function hook.Remove(ev, id) if hook.t[ev] then hook.t[ev][id] = nil end end
function hook.Run(ev, ...) for _, f in pairs(hook.t[ev] or {}) do local r = f(...) if r ~= nil then return r end end end
timer = { t = {} }
function timer.Create(n, d, r, f) timer.t[n] = { d = d, r = r, f = f } end
function timer.Remove(n) timer.t[n] = nil end
"#;

#[test]
fn attrs_register_and_teardown() {
    let a = common::api();
    let l = unsafe {
        let l = (a.luaL_newstate)();
        (a.luaL_openlibs)(l);
        l
    };
    common::exec(l, MOCK).unwrap();
    assert_eq!(unsafe { module::open(l, &INFO) }, 0);
    assert_eq!(common::exec(l, "return hook.Run('PlayerSay', nil, '!ping')").unwrap(), "pong");
    common::exec(l, "hook.Run('MyAddon.Event', 7)").unwrap();
    assert_eq!(common::exec(l, "return event_a").unwrap(), "7");
    assert_eq!(common::exec(l, "local t = timer.t['rsgdll.h.tick'] t.f() return t.d .. ':' .. t.r").unwrap(), "1:0");
    assert_eq!(common::exec(l, "return tostring(ticked)").unwrap(), "true");
    // Think pump hook is registered too (hook.Add exists in the mock).
    assert_eq!(common::exec(l, "return tostring(hook.t.Think['rsgdll.h.pump'] ~= nil)").unwrap(), "true");
    unsafe { module::close(l) };
    let left = common::exec(l, "local n = 0 for _, ev in pairs(hook.t) do for _ in pairs(ev) do n = n + 1 end end for _ in pairs(timer.t) do n = n + 1 end return n").unwrap();
    assert_eq!(left, "0", "framework hooks/timers must be removed at close");
}

#[rsgdll::hook(PlayerSay)]
fn on_say_compat(_ent: Option<EntityRef<'_>>, _text: LuaValue) -> Result<()> {
    Ok(())
}
