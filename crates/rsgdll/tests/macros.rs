//! Macro expansions end to end on stock LuaJIT (spec §8): exports, lifecycle, lua!, methods.

mod common;

use std::cell::RefCell;

use rsgdll::__rt::module::{self, ModuleInfo};
use rsgdll::Realm;
use rsgdll::prelude::*;

thread_local! {
    static LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}
fn log(s: &'static str) {
    LOG.with(|l| l.borrow_mut().push(s));
}

#[rsgdll::preopen]
fn pre(_: &Lua) -> Result<()> {
    log("preopen");
    Ok(())
}

#[rsgdll::postopen(order = 1)]
fn post(lua: &Lua) -> Result<()> {
    log("postopen");
    lua.global::<LuaFunction>("t.Add").map(|_| ())
}

#[rsgdll::preclose]
fn preclose(_: &Lua) {
    log("preclose");
}

#[rsgdll::postclose]
fn postclose() {
    log("postclose");
}

#[rsgdll::function]
fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[rsgdll::function]
fn divide(a: f64, b: f64) -> Result<f64> {
    (b != 0.0)
        .then(|| a / b)
        .ok_or_else(|| anyhow!("division by zero"))
}

#[rsgdll::function(name = "t.math.Clamp")]
fn clamp(v: f64, lo: Option<f64>, hi: Option<f64>) -> f64 {
    v.max(lo.unwrap_or(f64::MIN)).min(hi.unwrap_or(f64::MAX))
}

#[rsgdll::function]
fn sum_all(first: i32, rest: Variadic<i32>) -> (i32, usize) {
    (first + rest.iter().sum::<i32>(), rest.len())
}

#[rsgdll::function]
fn parse_num(s: &str) -> std::result::Result<i32, std::num::ParseIntError> {
    s.parse()
}

#[rsgdll::function]
fn boom() -> i32 {
    panic!("kaboom")
}

#[rsgdll::function]
fn via_lua(lua: &Lua, x: i32) -> Result<i32> {
    let doubled: i32 = lua!(lua, return $x * 2)?;
    let (a, b): (i32, String) = lua!(
        lua,
        r#"
        local n = 0
        for i = 1, 3 do if i == 2 then continue end n = n + i end
        return n + $y, "s" .. $x
    "#,
        y = 10
    )?;
    Ok(doubled + a + b.len() as i32)
}

#[rsgdll::function]
fn lua_error(lua: &Lua) -> Result<()> {
    lua!(lua, error("inner failure"))
}

#[rsgdll::function(realm = "server")]
fn server_only() -> bool {
    true
}

#[derive(LuaUserData)]
struct Counter {
    n: i64,
}

#[rsgdll::methods]
impl Counter {
    fn inc(&mut self, by: Option<i64>) -> i64 {
        self.n += by.unwrap_or(1);
        self.n
    }
    fn __tostring(&self) -> String {
        format!("Counter({})", self.n)
    }
}

#[rsgdll::function]
fn new_counter() -> LuaUd<Counter> {
    LuaUd(Counter { n: 0 })
}

static INFO: ModuleInfo = ModuleInfo {
    name: "t",
    realms: &[Realm::Server],
    gmod: false,
};

fn open() -> *mut rsgdll::__rt::sys::lua_State {
    let a = common::api();
    unsafe {
        let l = (a.luaL_newstate)();
        (a.luaL_openlibs)(l);
        assert_eq!(module::open(l, &INFO), 0);
        l
    }
}

#[test]
fn exports_and_lifecycle() {
    let l = open();
    assert_eq!(common::exec(l, "return t.Add(1, 2)").unwrap(), "3");
    assert_eq!(common::exec(l, "return t.Divide(4, 2)").unwrap(), "2");
    let e = common::exec(
        l,
        "local ok, e = pcall(t.Divide, 1, 0) return tostring(ok) .. e",
    )
    .unwrap();
    assert!(e.starts_with("false[t] t.Divide: division by zero"), "{e}");
    assert!(e.contains("rust stack traceback"), "{e}");
    let e = common::exec(l, "return t.Add('x')").unwrap_err();
    assert!(
        e.contains("bad argument #1 to 't.Add' (number expected, got string)"),
        "{e}"
    );
    assert_eq!(
        common::exec(l, "return t.math.Clamp(5, 0, 3)").unwrap(),
        "3"
    );
    assert_eq!(common::exec(l, "return t.math.Clamp(-1, 0)").unwrap(), "0");
    assert_eq!(
        common::exec(l, "local s, n = t.SumAll(1, 2, 3) return s .. ':' .. n").unwrap(),
        "6:2"
    );
    assert!(
        common::exec(l, "return t.ParseNum('zz')")
            .unwrap_err()
            .contains("invalid digit")
    );
    let e = common::exec(l, "local ok, e = pcall(t.Boom) return e").unwrap();
    assert!(e.contains("rust panic") && e.contains("kaboom"), "{e}");
    assert_eq!(common::exec(l, "return t.Add(2, 2)").unwrap(), "4");
    assert_eq!(
        common::exec(l, "return tostring(t.ServerOnly())").unwrap(),
        "true"
    );
    unsafe { module::close(l) };
    LOG.with(|g| {
        assert_eq!(
            *g.borrow(),
            ["preopen", "postopen", "preclose", "postclose"]
        )
    });
}

#[test]
fn lua_macro_and_methods() {
    let l = open();
    // 6 + (1 + 3 + 10) + len("s3") = 22
    assert_eq!(common::exec(l, "return t.ViaLua(3)").unwrap(), "22");
    let e = common::exec(l, "return t.LuaError()").unwrap_err();
    assert!(
        e.contains("inner failure") && e.contains("lua! at") && e.contains("stack traceback"),
        "{e}"
    );
    assert_eq!(
        common::exec(
            l,
            "local c = t.NewCounter() c:Inc() return c:Inc(4) .. tostring(c)"
        )
        .unwrap(),
        "5Counter(5)"
    );
}
