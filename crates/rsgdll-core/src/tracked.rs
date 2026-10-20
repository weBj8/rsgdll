//! Framework-created hooks/timers, removed at close and on failed open (spec §10.3).

use crate::conv::LuaFunction;
use crate::lua::Lua;

/// Records a hook created by the framework.
pub fn hook(lua: &Lua, event: &str, id: &str) {
    lua.runtime()
        .hooks
        .borrow_mut()
        .push((event.to_owned(), id.to_owned()));
}

pub fn untrack_hook(lua: &Lua, event: &str, id: &str) {
    lua.runtime()
        .hooks
        .borrow_mut()
        .retain(|(e, i)| !(e == event && i == id));
}

pub fn timer(lua: &Lua, name: &str) {
    lua.runtime().timers.borrow_mut().push(name.to_owned());
}

pub fn untrack_timer(lua: &Lua, name: &str) {
    lua.runtime().timers.borrow_mut().retain(|n| n != name);
}

/// Calls `hook.Remove` / `timer.Remove` for everything tracked. Failures are logged.
pub fn teardown(lua: &Lua) {
    let hooks: Vec<(String, String)> = lua.runtime().hooks.borrow_mut().drain(..).collect();
    let timers: Vec<String> = lua.runtime().timers.borrow_mut().drain(..).collect();
    let log = |what: &str, e: crate::Error| {
        rsgdll_sys::tier0::msg(&format!("[{}] cleanup {what}: {e}\n", lua.module_name()))
    };
    if let Ok(Some(f)) = lua.global::<Option<LuaFunction>>("hook.Remove") {
        hooks.iter().for_each(|(e, i)| {
            if let Err(err) = f.call::<_, ()>(lua, (e.as_str(), i.as_str())) {
                log(&format!("hook {e}/{i}"), err);
            }
        })
    }
    if let Ok(Some(f)) = lua.global::<Option<LuaFunction>>("timer.Remove") {
        timers.iter().for_each(|n| {
            if let Err(err) = f.call::<_, ()>(lua, n.as_str()) {
                log(&format!("timer {n}"), err);
            }
        })
    }
}
