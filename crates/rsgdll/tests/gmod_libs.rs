//! hook/timer wrappers against a Lua mock of GMod's libraries (spec §10.3/10.4).

mod common;

use std::time::Duration;

use rsgdll::{hook, hooks, timer};

const MOCK: &str = r#"
hook = { t = {} }
function hook.Add(ev, id, f) hook.t[ev] = hook.t[ev] or {} hook.t[ev][id] = f end
function hook.Remove(ev, id) if hook.t[ev] then hook.t[ev][id] = nil end end
function hook.Run(ev, ...) for _, f in pairs(hook.t[ev] or {}) do local r = f(...) if r ~= nil then return r end end end
function hook.Call(ev, gm, ...) return hook.Run(ev, ...) end
timer = { t = {} }
function timer.Create(n, d, r, f) timer.t[n] = { d = d, r = r, f = f } end
function timer.Simple(d, f) f() end
function timer.Adjust(n, d, r) if timer.t[n] then timer.t[n].d = d timer.t[n].r = r return true end return false end
function timer.Remove(n) timer.t[n] = nil end
function timer.Exists(n) return timer.t[n] ~= nil end
"#;

#[test]
fn typed_and_custom_hooks() {
    let (l, lua) = common::state();
    common::exec(l, MOCK).unwrap();
    // Typed built-in (HRTB over GAT closure): params inferred from hooks::PlayerSay.
    hook::add::<hooks::PlayerSay, _>(&lua, "t.chat", |_, (_ply, text, _team)| Ok(text.starts_with("!ping").then(|| "pong".to_owned()))).unwrap();
    assert_eq!(common::exec(l, "return hook.Run('PlayerSay', nil, '!ping', false)").unwrap(), "pong");
    assert_eq!(common::exec(l, "return tostring((hook.Run('PlayerSay', nil, 'hi', false)))").unwrap(), "nil");
    let r: Option<String> = hook::run::<hooks::PlayerSay>(&lua, (None, "!ping x".into(), false)).unwrap();
    assert_eq!(r.as_deref(), Some("pong"));
    hook::add_custom(&lua, "MyEvent", "id", |_, (a, b): (i32, String)| Ok(format!("{a}{b}"))).unwrap();
    let r: String = hook::run_custom(&lua, "MyEvent", (1, "x")).unwrap();
    assert_eq!(r, "1x");
    assert_eq!(lua.runtime().hooks.borrow().len(), 2);
    hook::remove(&lua, "MyEvent", "id").unwrap();
    assert_eq!(lua.runtime().hooks.borrow().len(), 1);
    // Close-time teardown removes the remaining tracked hook.
    rsgdll::__rt::tracked::teardown(&lua);
    assert_eq!(common::exec(l, "return tostring(next(hook.t.PlayerSay))").unwrap(), "nil");
}

#[test]
fn timers() {
    let (l, lua) = common::state();
    common::exec(l, MOCK).unwrap();
    timer::create(&lua, "t.tick", Duration::from_secs(1), timer::Repeat::Times(3), |lua| lua.set_global("ran", true)).unwrap();
    assert!(timer::exists(&lua, "t.tick").unwrap());
    assert!(timer::adjust(&lua, "t.tick", Duration::from_secs(2), timer::Repeat::Forever).unwrap());
    assert_eq!(common::exec(l, "return timer.t['t.tick'].d .. ':' .. timer.t['t.tick'].r").unwrap(), "2:0");
    common::exec(l, "timer.t['t.tick'].f()").unwrap();
    assert_eq!(common::exec(l, "return tostring(ran)").unwrap(), "true");
    timer::simple(&lua, Duration::from_millis(1), |lua| lua.set_global("simple", 1)).unwrap();
    assert_eq!(common::exec(l, "return simple").unwrap(), "1");
    timer::remove(&lua, "t.tick").unwrap();
    assert!(!timer::exists(&lua, "t.tick").unwrap());
}

rsgdll::hook_marker!(Chat2 = "PlayerSay": (Option<rsgdll::PlayerRef<'a>>, String) -> Option<String>);

#[test]
fn custom_marker_with_borrowed_args() {
    let (l, lua) = common::state();
    common::exec(l, MOCK).unwrap();
    hook::add::<Chat2, _>(&lua, "t.chat2", |_, (ply, text)| Ok((ply.is_none() && text.starts_with("!ping")).then(|| "pong".into()))).unwrap();
    assert_eq!(common::exec(l, "return hook.Run('PlayerSay', nil, '!ping')").unwrap(), "pong");
    let r = hook::run::<Chat2>(&lua, (None, "!ping".into())).unwrap();
    assert_eq!(r.as_deref(), Some("pong"));
}
