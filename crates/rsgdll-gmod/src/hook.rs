//! `hook` library wrappers (spec §10.3, §10.4). Calls the real Lua `hook.*` functions, so
//! behaviour matches GLua; framework-created ids are tracked and removed at close.

use rsgdll_core::conv::func::{bad_argument, push_raw_closure, returns};
use rsgdll_core::conv::{Chain, FromArgs, FromLuaMulti, IntoLuaMulti, LuaFunction, LuaTable};
use rsgdll_core::refs::LuaRef;
use rsgdll_core::trampoline::{self, Outcome};
use rsgdll_core::{Lua, Realm, Result, tracked};

/// A built-in hook marker (generated in [`crate::hooks`]).
pub trait Hook {
    const NAME: &'static str;
    /// `None` = shared.
    const REALM: Option<Realm>;
    const ARITY: usize;
    type Args<'a>: FromArgs<'a> + IntoLuaMulti;
    type Ret: IntoLuaMulti + FromLuaMulti;
}

/// Wraps a callback converting its args with `conv`; errors are raised into the hook runner.
fn callback(lua: &Lua, path: String, f: impl Fn(&Lua, i32) -> std::result::Result<Outcome, (i32, rsgdll_core::ConvError)> + 'static) -> Result<LuaFunction> {
    let boxed: Box<dyn Fn(&Lua) -> Outcome> = Box::new(move |lua| {
        let count = lua.top();
        f(lua, count).unwrap_or_else(|(n, e)| bad_argument(lua, &path, n, &e))
    });
    let g = lua.guard();
    push_raw_closure(lua, boxed)?;
    let r = LuaRef::new(lua, -1).map(LuaFunction);
    drop(g);
    r
}

fn outcome<R: IntoLuaMulti>(lua: &Lua, count: i32, path: &str, r: Result<R>) -> Outcome {
    match r {
        Ok(v) => returns(lua, count, path, v),
        Err(e) => Outcome::Err(trampoline::format_error(lua.module_name(), path, &e.lua_message())),
    }
}

fn hook_fn(lua: &Lua, name: &str) -> Result<LuaFunction> {
    lua.global::<Option<LuaFunction>>(&format!("hook.{name}"))?.ok_or_else(|| rsgdll_core::anyhow!("hook.{name} is not available"))
}

/// `hook.Add(event, id, f)` with a ready Lua function; tracked for removal at close.
pub fn add_function(lua: &Lua, event: &str, id: &str, f: LuaFunction) -> Result<()> {
    hook_fn(lua, "Add")?.call::<_, ()>(lua, (event, id, f))?;
    tracked::hook(lua, event, id);
    Ok(())
}

/// Custom/addon hook with string name; args/returns via `FromArgs`/`IntoLuaMulti`.
// shortcut: `A` is one type, so borrowed args (`PlayerRef<'_>`) need the typed `add::<H>`
// or `#[rsgdll::hook]`; use owned `Entity`/`Player` in custom closures.
pub fn add_custom<A, R, F>(lua: &Lua, event: &str, id: &str, f: F) -> Result<()>
where
    A: for<'a> FromArgs<'a>,
    R: IntoLuaMulti,
    F: Fn(&Lua, A) -> Result<R> + 'static,
{
    let path = format!("hook {event}/{id}");
    let p2 = path.clone();
    let cb = callback(lua, path, move |lua, count| A::from_args(lua, 1, count).map(|a| outcome(lua, count, &p2, f(lua, a))))?;
    add_function(lua, event, id, cb)
}

/// Typed built-in hook: wrong types/arity fail to compile (spec §10.4).
pub fn add<H, F>(lua: &Lua, id: &str, f: F) -> Result<()>
where
    H: Hook,
    F: for<'a> Fn(&'a Lua, H::Args<'a>) -> Result<H::Ret> + 'static,
{
    let path = format!("hook {}/{id}", H::NAME);
    let p2 = path.clone();
    let cb = callback(lua, path, move |lua, count| H::Args::from_args(lua, 1, count).map(|a| outcome(lua, count, &p2, f(lua, a))))?;
    add_function(lua, H::NAME, id, cb)
}

/// Positional-conversion variant used by `#[rsgdll::hook]` (the macro converts params).
#[doc(hidden)]
pub fn add_raw(lua: &Lua, event: &str, id: &str, f: impl Fn(&Lua, i32) -> std::result::Result<Outcome, (i32, rsgdll_core::ConvError)> + 'static) -> Result<()> {
    let cb = callback(lua, format!("hook {event}/{id}"), f)?;
    add_function(lua, event, id, cb)
}

/// `hook.Remove(event, id)`.
pub fn remove(lua: &Lua, event: &str, id: &str) -> Result<()> {
    hook_fn(lua, "Remove")?.call::<_, ()>(lua, (event, id))?;
    tracked::untrack_hook(lua, event, id);
    Ok(())
}

/// `hook.Run(event, args...)`.
pub fn run_custom<A: IntoLuaMulti, R: FromLuaMulti>(lua: &Lua, event: &str, args: A) -> Result<R> {
    hook_fn(lua, "Run")?.call(lua, Chain(event, args))
}

/// Typed `hook.Run`.
pub fn run<'a, H: Hook>(lua: &Lua, args: H::Args<'a>) -> Result<H::Ret> {
    run_custom(lua, H::NAME, args)
}

/// `hook.Call(event, gm, args...)`.
pub fn call_custom<A: IntoLuaMulti, R: FromLuaMulti>(lua: &Lua, event: &str, gm: Option<&LuaTable>, args: A) -> Result<R> {
    hook_fn(lua, "Call")?.call(lua, Chain((event, gm), args))
}

/// Typed `hook.Call`.
pub fn call<'a, H: Hook>(lua: &Lua, gm: Option<&LuaTable>, args: H::Args<'a>) -> Result<H::Ret> {
    call_custom(lua, H::NAME, gm, args)
}

/// `#[rsgdll::hook(Ident)]` param compatibility with the hook's arg type (spec §10.4):
/// identity, supertype refs (`Player` arg accepts `EntityRef`), any numeric for `number`,
/// and `LuaValue` for anything.
pub trait HookArgCompat<A> {}

impl<T> HookArgCompat<T> for T {}
impl<'a, 'b> HookArgCompat<Option<rsgdll_core::conv::PlayerRef<'a>>> for Option<rsgdll_core::conv::EntityRef<'b>> {}

macro_rules! numeric_compat {
    ($($t:ty),*) => {$( impl HookArgCompat<f64> for $t {} )*};
}
numeric_compat!(f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

macro_rules! value_compat {
    ($($t:ty),*) => {$( impl HookArgCompat<$t> for rsgdll_core::conv::LuaValue {} )*};
}
value_compat!(f64, String, bool, rsgdll_core::conv::LuaTable, rsgdll_core::conv::Vector, rsgdll_core::conv::Angle, rsgdll_core::conv::Color);
impl<'a> HookArgCompat<Option<rsgdll_core::conv::PlayerRef<'a>>> for rsgdll_core::conv::LuaValue {}
impl<'a> HookArgCompat<Option<rsgdll_core::conv::EntityRef<'a>>> for rsgdll_core::conv::LuaValue {}

/// Compile-time check used by the attribute macro.
#[doc(hidden)]
pub fn compat<P: HookArgCompat<A>, A>(_: &A) {}

/// Declares a typed marker for a custom/addon hook, so borrowed args (`PlayerRef<'a>`) work
/// with [`add`] / [`run`] (spec §10.3, decisions log §14). `'a` is the per-call lifetime.
///
/// ```ignore
/// rsgdll::hook_marker!(pub MyEvent = "MyAddon.Event": (Option<PlayerRef<'a>>, String) -> Option<String>);
/// hook::add::<MyEvent, _>(lua, "id", |_, (ply, text)| Ok(None))?;
/// ```
#[macro_export]
macro_rules! hook_marker {
    ($vis:vis $marker:ident = $name:literal : ($($arg:ty),* $(,)?) -> $ret:ty) => {
        $vis struct $marker;
        impl $crate::hook::Hook for $marker {
            const NAME: &'static str = $name;
            const REALM: ::core::option::Option<$crate::__Realm> = ::core::option::Option::None;
            const ARITY: usize = [$(::core::stringify!($arg)),*].len();
            type Args<'a> = ($($arg,)*);
            type Ret = $ret;
        }
    };
}
