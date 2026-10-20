//! Round trips of §6.2 conversions on stock LuaJIT.

mod common;

use std::collections::{BTreeMap, HashMap};

use rsgdll_core::*;

fn roundtrip<T: IntoLua + FromLua>(lua: &Lua, v: T) -> T {
    let g = lua.guard();
    lua.push(v).unwrap();
    let out = lua.get::<T>(-1).unwrap();
    drop(g);
    out
}

#[test]
fn primitives() {
    let (_l, lua) = common::state();
    assert_eq!(roundtrip(&lua, 42i64), 42);
    assert_eq!(roundtrip(&lua, 1.5f64), 1.5);
    assert!(roundtrip(&lua, true));
    assert_eq!(roundtrip(&lua, "héllo".to_owned()), "héllo");
    assert_eq!(roundtrip(&lua, Some(3u8)), Some(3));
    assert_eq!(roundtrip::<Option<u8>>(&lua, None), None);
    assert_eq!(roundtrip(&lua, AsString(76561198000000000u64) as SteamId64).0, 76561198000000000);
    assert_eq!(roundtrip(&lua, LuaBytes(vec![0, 255, 1])).0, vec![0, 255, 1]);
}

#[test]
fn int_limits() {
    let (_l, lua) = common::state();
    assert!(lua.push(1i64 << 54).is_err());
    let g = lua.guard();
    lua.push(2.5f64).unwrap();
    assert!(lua.get::<i32>(-1).is_err());
    lua.push(300.0f64).unwrap();
    assert!(lua.get::<u8>(-1).is_err());
    drop(g);
}

#[test]
fn number_string_coercion_and_utf8() {
    let (l, lua) = common::state();
    let g = lua.guard();
    lua.push(5.0f64).unwrap();
    assert_eq!(lua.get::<String>(-1).unwrap(), "5");
    lua.push("7").unwrap();
    assert_eq!(lua.get::<i32>(-1).unwrap(), 7);
    common::exec(l, "bad = '\\255\\254'").unwrap();
    let e = lua.global::<String>("bad").unwrap_err();
    assert!(e.to_string().contains("expected UTF-8 string"), "{e}");
    drop(g);
}

#[test]
fn tables() {
    let (l, lua) = common::state();
    assert_eq!(roundtrip(&lua, vec![1, 2, 3]), vec![1, 2, 3]);
    assert_eq!(roundtrip(&lua, [1u8, 2]), [1, 2]);
    let m: HashMap<String, i32> = [("a".to_owned(), 1), ("b".to_owned(), 2)].into();
    assert_eq!(roundtrip(&lua, m.clone()), m);
    let b: BTreeMap<i32, String> = [(1, "x".to_owned())].into();
    assert_eq!(roundtrip(&lua, b.clone()), b);
    common::exec(l, "holes = {1, nil, 3, 4}").unwrap();
    let e = lua.global::<Vec<i32>>("holes").unwrap_err();
    assert!(e.to_string().contains("hole at 2"), "{e}");
    common::exec(l, "nested = { a = { 1, 'x' } }").unwrap();
    let e = lua.global::<HashMap<String, Vec<i32>>>("nested").unwrap_err();
    assert!(e.to_string().contains("field 'a.2'"), "{e}");
}

#[test]
fn metamethods_are_honoured() {
    let (l, lua) = common::state();
    common::exec(l, "proxy = setmetatable({}, { __index = function(_, k) return k * 10 end })").unwrap();
    let t: LuaTable = lua.global("proxy").unwrap();
    assert_eq!(t.get::<_, i32>(&lua, 4).unwrap(), 40);
}

#[test]
fn functions_and_closures() {
    let (l, lua) = common::state();
    common::exec(l, "function add(a, b) return a + b, 'ok' end").unwrap();
    let f: LuaFunction = lua.global("add").unwrap();
    let (n, s): (i32, String) = f.call(&lua, (2, 3)).unwrap();
    assert_eq!((n, s.as_str()), (5, "ok"));
    let double = lua.create_function(|_, x: i32| Ok(x * 2)).unwrap();
    lua.set_global("double", double).unwrap();
    assert_eq!(common::exec(l, "return double(21)").unwrap(), "42");
    let e = common::exec(l, "return double('x')").unwrap_err();
    assert!(e.contains("bad argument #1"), "{e}");
    let failing = lua.create_function(|_, ()| -> Result<()> { Err(anyhow!("nope")) }).unwrap();
    lua.set_global("failing", failing).unwrap();
    assert!(common::exec(l, "return failing()").unwrap_err().contains("nope"));
}

#[test]
fn nested_globals() {
    let (l, lua) = common::state();
    lua.set_global("a.b.c", 5).unwrap();
    assert_eq!(common::exec(l, "return a.b.c").unwrap(), "5");
    assert_eq!(lua.global::<i32>("a.b.c").unwrap(), 5);
}

#[test]
fn lua_value() {
    let (l, lua) = common::state();
    common::exec(l, "v = { 1 }").unwrap();
    assert!(matches!(lua.global::<LuaValue>("v").unwrap(), LuaValue::Table(_)));
    assert_eq!(lua.global::<LuaValue>("nothing").unwrap().is_nil(), true);
}

struct Counter {
    n: i64,
    dropped: std::rc::Rc<std::cell::Cell<bool>>,
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

impl UserData for Counter {
    const NAME: &'static str = "Counter";
    fn methods(m: &mut UserDataMethods<Self>) {
        m.method_mut("Inc", |_, c, (by,): (Option<i64>,)| {
            c.n += by.unwrap_or(1);
            Ok(c.n)
        });
        m.meta("__tostring", |_, c| Ok(format!("Counter({})", c.n)));
    }
}

#[test]
fn userdata_methods_and_gc() {
    let (l, lua) = common::state();
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
    lua.set_global("c", LuaUd(Counter { n: 0, dropped: dropped.clone() })).unwrap();
    assert_eq!(common::exec(l, "c:Inc() return c:Inc(5)").unwrap(), "6");
    assert_eq!(common::exec(l, "return tostring(c)").unwrap(), "Counter(6)");
    assert!(common::exec(l, "return c.Inc({})").unwrap_err().contains("bad argument #1"));
    common::exec(l, "c = nil collectgarbage() collectgarbage()").unwrap();
    assert!(dropped.get());
}

#[cfg(feature = "serde")]
#[test]
fn serde_roundtrip() {
    use rsgdll_core::conv::serde::LuaSerde;
    #[derive(::serde::Serialize, ::serde::Deserialize, Debug, PartialEq, Clone)]
    enum Kind { A, B { n: u32 } }
    #[derive(::serde::Serialize, ::serde::Deserialize, Debug, PartialEq, Clone)]
    struct Cfg { name: String, tags: Vec<String>, kind: Kind, opt: Option<i32> }
    let (_l, lua) = common::state();
    let v = Cfg { name: "x".into(), tags: vec!["a".into()], kind: Kind::B { n: 3 }, opt: None };
    assert_eq!(roundtrip(&lua, LuaSerde(v.clone())).0, v);
    assert_eq!(roundtrip(&lua, LuaSerde(Kind::A)).0, Kind::A);
}
