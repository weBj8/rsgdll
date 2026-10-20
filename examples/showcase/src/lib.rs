//! Exercises every rsgdll feature (spec §13.2); doubles as the srcds feature test suite.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rsgdll::prelude::*;
use rsgdll::{LuaSerde, Realm, hook, hooks, timer};

rsgdll::module!(name = "showcase");

static LIFECYCLE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static THINKS: AtomicUsize = AtomicUsize::new(0);
static TICKS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);
/// Handle kept across a map change to observe `StateClosed`.
static OLD_HANDLE: Mutex<Option<LuaHandle>> = Mutex::new(None);

fn note(s: &'static str) {
    if let Ok(mut v) = LIFECYCLE.lock() {
        v.push(s);
    }
}

#[rsgdll::preopen]
fn preopen(_: &Lua) -> Result<()> {
    note("preopen");
    Ok(())
}

#[rsgdll::postopen]
fn postopen(lua: &Lua) -> Result<()> {
    note("postopen");
    lua.print("showcase: open")
}

#[rsgdll::preclose]
fn preclose(lua: &Lua) {
    note("preclose");
    let _ = lua.print("showcase: preclose");
}

#[rsgdll::postclose]
fn postclose() {
    note("postclose");
    eprintln!("showcase: postclose");
}

#[rsgdll::function]
fn lifecycle() -> Vec<String> {
    LIFECYCLE.lock().map(|v| v.iter().map(|s| s.to_string()).collect()).unwrap_or_default()
}

// ---- exports ----
#[rsgdll::function]
fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[rsgdll::function(name = "ShowcaseGlobal")]
fn global_fn() -> &'static str {
    "global"
}

#[rsgdll::function(name = "showcase.math.Clamp")]
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.clamp(lo, hi)
}

#[rsgdll::function(realm = "server")]
fn server_only() -> bool {
    true
}

#[rsgdll::function]
fn sum(first: f64, rest: Variadic<f64>) -> f64 {
    first + rest.iter().sum::<f64>()
}

#[rsgdll::function]
fn greet_opt(name: Option<String>) -> String {
    format!("hi {}", name.as_deref().unwrap_or("anon"))
}

#[rsgdll::function]
fn div_mod(a: i64, b: i64) -> Result<(i64, i64)> {
    (b != 0).then(|| (a / b, a % b)).ok_or_else(|| anyhow!("division by zero"))
}

// ---- conversion ----
#[derive(LuaValue, Clone, Debug, PartialEq)]
#[lua(rename_all = "PascalCase")]
struct Config {
    name: String,
    #[lua(default)]
    retries: u32,
    tags: Vec<String>,
}

#[derive(LuaValue, Clone, Debug, PartialEq)]
#[lua(tag = "kind")]
enum Action {
    Kick { reason: String },
    Ban { minutes: u32 },
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Point {
    x: f64,
    y: f64,
}

#[rsgdll::function]
fn echo_config(c: Config) -> Config {
    Config { retries: c.retries + 1, ..c }
}

#[rsgdll::function]
fn echo_action(a: Action) -> Action {
    a
}

#[rsgdll::function]
fn serde_point(p: LuaSerde<Point>) -> LuaSerde<Point> {
    LuaSerde(Point { x: p.0.y, y: p.0.x })
}

#[rsgdll::function]
fn vec_add(a: Vector, b: Vector) -> Vector {
    Vector { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z }
}

#[rsgdll::function]
fn ang_flip(a: Angle) -> Angle {
    Angle { p: -a.p, y: -a.y, r: -a.r }
}

#[rsgdll::function]
fn red() -> Color {
    Color { r: 255, g: 0, b: 0, a: 255 }
}

#[rsgdll::function]
fn ent_class(e: EntityRef<'_>) -> Result<String> {
    e.call("GetClass", ())
}

#[rsgdll::function]
fn maybe_player(p: Option<PlayerRef<'_>>) -> bool {
    p.is_some()
}

#[rsgdll::function]
fn too_big() -> i64 {
    1 << 54
}

// ---- errors ----
#[rsgdll::function]
fn boom() -> i32 {
    panic!("showcase panic")
}

#[rsgdll::function]
fn lua_error_text(lua: &Lua) -> Result<String> {
    let r: Result<()> = lua!(lua, error("inner"));
    Ok(r.err().map(|e| format!("{e:?}")).unwrap_or_default())
}

// ---- userdata ----
#[derive(LuaUserData)]
struct Counter {
    n: i64,
}

impl Drop for Counter {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
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

#[rsgdll::function]
fn drops() -> usize {
    DROPS.load(Ordering::SeqCst)
}

// ---- lua! ----
#[rsgdll::function]
fn lua_forms(lua: &Lua, x: i32) -> Result<(i32, String, i32)> {
    let n: i32 = lua!(lua, math.max($x, 3) + 1)?;
    let (s, k): (String, i32) = lua!(lua, r#"
        local acc = 0
        for i = 1, 5 do if i == 3 then continue end acc = acc + i end
        return "x" .. $x, acc
    "#)?;
    Ok((n, s, k))
}

// ---- interfaces ----
#[rsgdll::function]
fn interfaces(lua: &Lua) -> Result<(bool, bool, String)> {
    let iface = lua.interface()?;
    iface.run_string("showcase", "", "SHOWCASE_RUNSTRING = 42")?;
    let shared = rsgdll::interfaces::lua_shared()?;
    let cross = shared.lua_interface(Realm::Server).ok_or_else(|| anyhow!("no server interface"))?;
    Ok((iface.is_server(), cross.is_server()?, iface.path_id()))
}

// ---- hooks & timers ----
#[rsgdll::hook(Think)]
fn on_think() -> Result<()> {
    THINKS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[rsgdll::hook("Showcase.Event")]
fn on_event(lua: &Lua, v: i32) -> Result<()> {
    lua.set_global("SHOWCASE_EVENT", v)
}

#[rsgdll::timer(every = "0.1s")]
fn tick() -> Result<()> {
    TICKS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[rsgdll::function]
fn counts() -> (usize, usize) {
    (THINKS.load(Ordering::Relaxed), TICKS.load(Ordering::Relaxed))
}

#[rsgdll::function]
fn hooks_and_timers(lua: &Lua) -> Result<Option<String>> {
    hook::add::<hooks::PlayerSay, _>(lua, "showcase.say", |_, (_p, text, _t)| Ok(text.starts_with("!ping").then(|| "pong".into())))?;
    let r = hook::run::<hooks::PlayerSay>(lua, (None, "!ping".into(), false))?;
    hook::remove(lua, "PlayerSay", "showcase.say")?;
    timer::simple(lua, Duration::from_millis(50), |lua| lua.set_global("SHOWCASE_SIMPLE", true))?;
    timer::create(lua, "showcase.rep", Duration::from_millis(50), timer::Repeat::Times(2), |lua| {
        let n: i32 = lua.global::<Option<i32>>("SHOWCASE_REP")?.unwrap_or(0);
        lua.set_global("SHOWCASE_REP", n + 1)
    })?;
    timer::adjust(lua, "showcase.rep", Duration::from_millis(60), timer::Repeat::Times(2))?;
    Ok(r)
}

// ---- async ----
#[rsgdll::function]
fn thread_call(lua: &Lua) {
    let h = lua.handle();
    *OLD_HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(h.clone());
    std::thread::spawn(move || h.call_blocking(|lua| lua.set_global("SHOWCASE_THREAD", "ok")));
}

#[rsgdll::function]
fn local_sleep(lua: &Lua) {
    rsgdll::spawn_local(lua, |lua| async move {
        rsgdll::sleep(Duration::from_millis(100)).await;
        let v = rsgdll::spawn_worker(|| Ok(6 * 7)).await?;
        lua!(&lua, SHOWCASE_LOCAL = $v)
    });
}

#[rsgdll::function]
async fn fetch(lua: LocalLua, x: i32) -> Result<String> {
    let y = rsgdll::spawn_worker(move || Ok(x * 2)).await?;
    rsgdll::sleep(Duration::from_millis(20)).await;
    lua!(&lua, return "v" .. $y)
}

/// After a map change: the handle from the old state reports `StateClosed`.
#[rsgdll::function]
fn stale_handle() -> String {
    let h = OLD_HANDLE.lock().ok().and_then(|g| g.clone());
    match h {
        None => "none".into(),
        Some(h) => std::thread::spawn(move || match h.call_blocking(|_| Ok(())) {
            Err(e) if matches!(e.kind(), ErrorKind::StateClosed) => "closed".to_owned(),
            other => format!("{:?}", other.err()),
        })
        .join()
        .unwrap_or_else(|_| "panic".into()),
    }
}

/// `probe-threadedcall`: `AddThreadedCall` from `n` worker threads (spec §10.2 gate).
#[rsgdll::function]
fn probe_threaded(lua: &Lua, n: usize) -> Result<()> {
    rsgdll::__rt::rt::threaded::probe(lua, n)
}

#[rsgdll::function]
fn probe_runs() -> usize {
    rsgdll::__rt::rt::threaded::PROBE_RUNS.load(Ordering::SeqCst)
}
