//! Module lifecycle (spec §8.1): `linkme` registries filled by the macros, and the
//! `gmod13_open` / `gmod13_close` bodies.

use core::ffi::c_int;

use linkme::distributed_slice;
use rsgdll_sys::lua::{CFunc, LUA_GLOBALSINDEX, lua_State};

use crate::error::{Error, Result};
use crate::lua::Lua;
use crate::realm::Realm;
use crate::trampoline;

/// One `#[rsgdll::function]` export.
pub struct Export {
    /// Name as written (`Add`) or explicit path (`Add`, `mymod.config.Load`).
    pub name: &'static str,
    /// `true` = relative to the module table (`<module>.<name>`).
    pub in_module: bool,
    /// `None` = every realm the DLL is opened in.
    pub realm: Option<Realm>,
    pub func: CFunc,
}

/// Ordered lifecycle callback.
pub struct Ordered<F> {
    pub order: i32,
    pub name: &'static str,
    pub f: F,
}

#[distributed_slice]
pub static EXPORTS: [Export];
#[distributed_slice]
pub static PREOPEN: [Ordered<fn(&Lua) -> Result<()>>];
#[distributed_slice]
pub static POSTOPEN: [Ordered<fn(&Lua) -> Result<()>>];
#[distributed_slice]
pub static PRECLOSE: [Ordered<fn(&Lua)>];
#[distributed_slice]
pub static POSTCLOSE: [Ordered<fn()>];
/// Framework registrations run in step 3 (hooks, timers, userdata metatables).
#[distributed_slice]
pub static REGISTER: [Ordered<fn(&Lua) -> Result<()>>];
/// Framework teardown run after `#[preclose]` (executor, pending calls), in order.
#[distributed_slice]
pub static TEARDOWN: [Ordered<fn(&Lua)>];

fn sorted<F>(items: &'static [Ordered<F>]) -> Vec<&'static Ordered<F>> {
    let mut v: Vec<_> = items.iter().collect();
    v.sort_by_key(|o| (o.order, o.name));
    v
}

/// Compile-time facts of the module crate, passed by `module!`.
pub struct ModuleInfo {
    pub name: &'static str,
    /// Realm features enabled in the user crate.
    pub realms: &'static [Realm],
    /// Running inside GMod (luabase/interface available). `false` only in tests.
    pub gmod: bool,
}

fn remove_exports(lua: &Lua, paths: &[String]) {
    paths.iter().for_each(|p| {
        if let Err(e) = lua.set_global(p.as_str(), crate::conv::Nil) {
            rsgdll_sys::tier0::msg(&format!("[{}] cleanup: failed to remove {p}: {e}\n", lua.module_name()));
        }
    });
}

/// Steps 2-4 of §8.1 after the runtime exists; on error cleans up and returns `Err`.
fn open_steps(lua: &Lua, info: &ModuleInfo) -> Result<()> {
    sorted(&PREOPEN).iter().try_for_each(|h| (h.f)(lua).map_err(|e| e.context(format!("preopen `{}`", h.name))))?;
    let realm = lua.realm();
    let mut registered: Vec<String> = Vec::new();
    let reg = EXPORTS
        .iter()
        .filter(|e| e.realm.is_none_or(|r| Some(r) == realm))
        .try_for_each(|e| {
            let path = crate::export::path(lua, e.name, e.in_module);
            let g = lua.guard();
            let r = lua
                .push_cfunction(e.func)
                .and_then(|()| crate::refs::LuaRef::new(lua, -1))
                .and_then(|f| lua.set_global(&path, crate::conv::LuaAny(f)));
            drop(g);
            r.map_err(|err| err.context(format!("registering `{path}`"))).map(|()| registered.push(path))
        })
        .and_then(|()| sorted(&REGISTER).iter().try_for_each(|r| (r.f)(lua)))
        .and_then(|()| sorted(&POSTOPEN).iter().try_for_each(|h| (h.f)(lua).map_err(|e| e.context(format!("postopen `{}`", h.name)))));
    let _ = info;
    reg.inspect_err(|_| {
        remove_exports(lua, &registered);
        crate::tracked::teardown(lua);
    })
}

#[inline(never)]
fn open_inner(l: *mut lua_State, info: &'static ModuleInfo) -> bool {
    if let Err(e) = rsgdll_sys::lua_shared() {
        rsgdll_sys::tier0::warning(&format!("[{}] cannot load: {e}\n", info.name));
        return false;
    }
    trampoline::install_panic_hook();
    rsgdll_sys::loader::pin_self(open as *const core::ffi::c_void);
    let api = rsgdll_sys::api();
    // SAFETY: pushthread uses one reserved slot and returns 1 only on the main thread.
    let main = unsafe {
        let m = (api.lua_pushthread)(l) == 1;
        (api.lua_settop)(l, -2);
        m
    };
    if !main {
        // SAFETY: static string; may raise on OOM, nothing with Drop is live in Lua's view.
        unsafe { (api.lua_pushstring)(l, c"rsgdll: modules must be required from the main thread, not a coroutine".as_ptr()) };
        return true;
    }
    // SAFETY: main thread verified above.
    let lua = match unsafe { crate::lua::open_state(l, info.name) } {
        Ok(lua) => lua,
        Err(e) => {
            rsgdll_sys::tier0::warning(&format!("[{}] bootstrap failed: {e}\n", info.name));
            return false;
        }
    };
    let setup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::interface::attach(&lua, info).and_then(|()| open_steps(&lua, info))
    }))
    .unwrap_or_else(|_| Err(Error::msg("panic during module open")));
    match setup {
        Ok(()) => false,
        Err(e) => {
            trampoline::push_message(&lua, &trampoline::format_error(info.name, "require", &e.lua_message()));
            // Failed open: drop the runtime now; GMod is not assumed to call gmod13_close.
            crate::lua::close_state(lua);
            true
        }
    }
}

/// Body of the generated `gmod13_open` (spec §8.1). Raises with nothing `Drop` alive.
///
/// # Safety
/// Called by GMod (or a test harness) with a live state.
pub unsafe fn open(l: *mut lua_State, info: &'static ModuleInfo) -> c_int {
    let raise: bool = open_inner(l, info);
    match raise {
        // SAFETY: message on top; only plain values in this frame.
        true => unsafe { (rsgdll_sys::api().lua_error)(l) },
        false => 0,
    }
}

/// Body of the generated `gmod13_close`.
///
/// # Safety
/// Called by GMod with the state previously passed to [`open`].
pub unsafe fn close(l: *mut lua_State) -> c_int {
    // SAFETY: a failed open removed its entry, so this is a no-op then.
    let Ok(lua) = (unsafe { Lua::from_raw(l) }) else { return 0 };
    sorted(&PRECLOSE).iter().for_each(|h| {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (h.f)(&lua)));
        if r.is_err() {
            rsgdll_sys::tier0::warning(&format!("[{}] preclose `{}` panicked\n", lua.module_name(), h.name));
        }
    });
    crate::tracked::teardown(&lua);
    sorted(&TEARDOWN).iter().for_each(|t| (t.f)(&lua));
    let _ = lua.flush_unrefs();
    crate::lua::close_state(lua);
    sorted(&POSTCLOSE).iter().for_each(|h| {
        let _ = std::panic::catch_unwind(h.f);
    });
    0
}

impl Lua {
    /// Pushes a C function (protected: allocates).
    pub fn push_cfunction(&self, f: CFunc) -> Result<()> {
        self.run(&[], &[crate::protected::Op::PushCClosure(f, 0)]).map(|_| ())
    }

    /// `_G` pseudo-index (for raw users).
    pub const GLOBALS: c_int = LUA_GLOBALSINDEX;
}

/// Shortcut used by exports to turn `Err` into `Error` for any `Display` (spec §7.1).
pub fn display_error(e: impl std::fmt::Display) -> Error {
    Error::msg(e)
}

/// Safe entry for the generated `gmod13_open` (user crates need no `unsafe`, spec §1).
#[doc(hidden)]
pub fn open_entry(l: *mut lua_State, info: &'static ModuleInfo) -> c_int {
    // SAFETY: only called from the generated `gmod13_open`, which GMod invokes with a live state.
    unsafe { open(l, info) }
}

/// Safe entry for the generated `gmod13_close`.
#[doc(hidden)]
pub fn close_entry(l: *mut lua_State) -> c_int {
    // SAFETY: only called from the generated `gmod13_close`.
    unsafe { close(l) }
}
