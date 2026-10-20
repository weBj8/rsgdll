//! `concommand`, convars, `net` helpers, printing (spec §10.3). Thin wrappers over the
//! real Lua libraries.

use rsgdll_core::conv::{FromLuaMulti, IntoLuaMulti, LuaAny, LuaBytes, LuaFunction, PlayerRef};
use rsgdll_core::{Lua, Result, anyhow};

fn lib_fn(lua: &Lua, path: &str) -> Result<LuaFunction> {
    lua.global::<Option<LuaFunction>>(path)?
        .ok_or_else(|| anyhow!("{path} is not available"))
}

/// Calls a library function by dotted path.
pub fn call<A: IntoLuaMulti, R: FromLuaMulti>(lua: &Lua, path: &str, args: A) -> Result<R> {
    lib_fn(lua, path)?.call(lua, args)
}

pub mod concommand {
    use super::*;

    /// `concommand.Add(name, f(ply, cmd, args, argStr), help)`; removed at close.
    pub fn add(
        lua: &Lua,
        name: &str,
        help: &str,
        f: impl Fn(&Lua, Option<PlayerRef<'_>>, Vec<String>, String) -> Result<()> + 'static,
    ) -> Result<()> {
        let path = format!("concommand {name}");
        let cb = rsgdll_core::conv::func::create_raw(lua, path, move |lua, count| {
            let ply = rsgdll_core::conv::multi::arg::<Option<PlayerRef<'_>>>(lua, 1, count, 0)?;
            let args = rsgdll_core::conv::multi::arg::<Option<Vec<String>>>(lua, 1, count, 2)?
                .unwrap_or_default();
            let line = rsgdll_core::conv::multi::arg::<Option<String>>(lua, 1, count, 3)?
                .unwrap_or_default();
            Ok(f(lua, ply, args, line))
        })?;
        call::<_, ()>(
            lua,
            "concommand.Add",
            (name, cb, rsgdll_core::conv::Nil, help),
        )?;
        lua.runtime()
            .ext::<std::cell::RefCell<Vec<String>>>("concommands", Default::default)
            .borrow_mut()
            .push(name.to_owned());
        Ok(())
    }

    pub fn remove(lua: &Lua, name: &str) -> Result<()> {
        call(lua, "concommand.Remove", name)
    }

    /// Removes every command registered through [`add`] (called at close).
    pub fn teardown(lua: &Lua) {
        let names: Vec<String> = lua
            .runtime()
            .ext::<std::cell::RefCell<Vec<String>>>("concommands", Default::default)
            .borrow_mut()
            .drain(..)
            .collect();
        names.iter().for_each(|n| {
            let _ = remove(lua, n);
        });
    }
}

pub mod cvars {
    use super::*;

    /// `CreateConVar(name, default, flags, help)`.
    pub fn create(lua: &Lua, name: &str, default: &str, flags: i32, help: &str) -> Result<LuaAny> {
        call(lua, "CreateConVar", (name, default, flags, help))
    }

    /// `GetConVar(name):GetString()`.
    pub fn get_string(lua: &Lua, name: &str) -> Result<Option<String>> {
        let cv: Option<LuaAny> = call(lua, "GetConVar", name)?;
        cv.map(|c| method(lua, &c, "GetString")).transpose()
    }

    pub fn get_number(lua: &Lua, name: &str) -> Result<Option<f64>> {
        let cv: Option<LuaAny> = call(lua, "GetConVar", name)?;
        cv.map(|c| method(lua, &c, "GetFloat")).transpose()
    }

    fn method<R: FromLuaMulti>(lua: &Lua, obj: &LuaAny, name: &str) -> Result<R> {
        let g = lua.guard();
        obj.0.push(lua)?;
        let r = rsgdll_core::conv::gmod::call_method(lua, -1, name, ());
        drop(g);
        r
    }
}

pub mod net {
    use super::*;

    /// `util.AddNetworkString(name)` (server).
    pub fn add_network_string(lua: &Lua, name: &str) -> Result<i32> {
        call(lua, "util.AddNetworkString", name)
    }

    /// `net.Start(name)` + `net.WriteData(bytes)` + send via `send` (`"Broadcast"`, ...).
    pub fn send_bytes(
        lua: &Lua,
        name: &str,
        data: &[u8],
        send: &str,
        target: Option<LuaAny>,
    ) -> Result<()> {
        call::<_, ()>(lua, "net.Start", name)?;
        call::<_, ()>(lua, "net.WriteUInt", (data.len() as u32, 32))?;
        call::<_, ()>(lua, "net.WriteData", (LuaBytes(data.to_vec()), data.len()))?;
        match target {
            Some(t) => call(lua, &format!("net.{send}"), t),
            None => call(lua, &format!("net.{send}"), ()),
        }
    }

    /// `net.Receive(name, f(len, ply))` reading the payload written by [`send_bytes`].
    pub fn receive_bytes(
        lua: &Lua,
        name: &str,
        f: impl Fn(&Lua, Vec<u8>, Option<PlayerRef<'_>>) -> Result<()> + 'static,
    ) -> Result<()> {
        let cb =
            rsgdll_core::conv::func::create_raw(lua, format!("net {name}"), move |lua, count| {
                let ply = rsgdll_core::conv::multi::arg::<Option<PlayerRef<'_>>>(lua, 1, count, 1)?;
                Ok(call::<_, u32>(lua, "net.ReadUInt", 32)
                    .and_then(|n| call::<_, LuaBytes>(lua, "net.ReadData", n))
                    .and_then(|b| f(lua, b.0, ply)))
            })?;
        call(lua, "net.Receive", (name, cb))
    }
}

/// `print(...)`.
pub fn print(lua: &Lua, msg: &str) -> Result<()> {
    lua.print(msg)
}

/// `Msg` through `ILuaInterface` (falls back to tier0).
pub fn msg(lua: &Lua, text: &str) {
    match lua.interface() {
        Ok(i) => i.msg(text),
        Err(_) => rsgdll_sys::tier0::msg(text),
    }
}

/// `ErrorNoHalt` (prints an error without raising).
pub fn error_no_halt(lua: &Lua, text: &str) {
    rsgdll_core::rt::executor::log_error(lua, text);
}
