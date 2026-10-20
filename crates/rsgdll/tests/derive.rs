//! `#[derive(LuaValue)]` / `#[derive(LuaUserData)]` on stock LuaJIT (spec §6.3).

mod common;

use rsgdll::prelude::*;

#[derive(LuaValue, Debug, PartialEq, Clone)]
#[lua(rename_all = "PascalCase")]
struct Config {
    name: String,
    #[lua(default)]
    retries: u32,
    #[lua(rename = "steamID")]
    steam_id: Option<String>,
    #[lua(skip)]
    cache: Vec<u8>,
    #[lua(as_string)]
    big: u64,
}

#[derive(LuaValue, Debug, PartialEq, Clone)]
#[lua(tag = "kind")]
enum Action {
    Kick { reason: String },
    Ban { minutes: u32 },
}

#[derive(LuaValue, Debug, PartialEq, Clone, Copy)]
enum Colour {
    Red,
    Green,
}

#[derive(LuaValue, Debug, PartialEq, Clone, Copy)]
#[lua(as_number)]
enum Level {
    Low = 1,
    High = 5,
}

#[derive(LuaUserData)]
struct Plain;

fn rt<T: rsgdll::IntoLua + rsgdll::FromLua>(lua: &Lua, v: T) -> T {
    let g = lua.guard();
    lua.push(v).unwrap();
    let out = lua.get::<T>(-1).unwrap();
    drop(g);
    out
}

#[test]
fn struct_roundtrip_and_attrs() {
    let (l, lua) = common::state();
    let c = Config { name: "a".into(), retries: 3, steam_id: Some("x".into()), cache: vec![1], big: 76561198000000000 };
    let back = rt(&lua, c.clone());
    assert_eq!(back, Config { cache: vec![], ..c });
    common::exec(l, "cfg = { Name = 'n', Big = '5' }").unwrap();
    let cfg: Config = lua.global("cfg").unwrap();
    assert_eq!((cfg.retries, cfg.steam_id, cfg.big), (0, None, 5));
    common::exec(l, "bad = { Name = 'n', Retries = 'x', Big = '1' }").unwrap();
    let e = lua.global::<Config>("bad").unwrap_err().to_string();
    assert!(e.contains("field 'Retries'") && e.contains("expected number, got string"), "{e}");
}

#[test]
fn enums() {
    let (l, lua) = common::state();
    assert_eq!(rt(&lua, Action::Ban { minutes: 5 }), Action::Ban { minutes: 5 });
    common::exec(l, "a = { kind = 'Kick', reason = 'spam' }").unwrap();
    assert_eq!(lua.global::<Action>("a").unwrap(), Action::Kick { reason: "spam".into() });
    assert_eq!(rt(&lua, Colour::Green), Colour::Green);
    common::exec(l, "c = 'Red'").unwrap();
    assert_eq!(lua.global::<Colour>("c").unwrap(), Colour::Red);
    assert_eq!(rt(&lua, Level::High), Level::High);
    common::exec(l, "lv = 1").unwrap();
    assert_eq!(lua.global::<Level>("lv").unwrap(), Level::Low);
}

#[test]
fn userdata_without_methods() {
    let (l, lua) = common::state();
    lua.set_global("p", LuaUd(Plain)).unwrap();
    assert_eq!(common::exec(l, "return type(p)").unwrap(), "userdata");
}
