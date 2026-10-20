//! Runtime helpers called by `#[rsgdll::function]` / `#[rsgdll::methods]` expansions.

use core::ffi::c_int;

use rsgdll_sys::lua::lua_State;

use crate::conv::func::{bad_argument, returns};
use crate::conv::IntoLuaMulti;
use crate::error::ConvError;
use crate::error::Error;
use crate::lua::Lua;
use crate::trampoline::{self, Outcome};

/// Full Lua path of an export (`<module>.<name>` for module-relative names).
pub fn path(lua: &Lua, name: &str, in_module: bool) -> String {
    match in_module {
        true => format!("{}.{name}", lua.module_name()),
        false => name.to_owned(),
    }
}

/// Failure while converting arguments: `(1-based arg number, error)`.
pub type ArgError = (c_int, ConvError);

/// Entry point for a generated trampoline. Safe wrapper so user crates with
/// `#![forbid(unsafe_code)]` need no `unsafe` block (spec §1).
#[doc(hidden)]
// Safe FFI boundary by design (spec §1): only macro-generated `extern "C-unwind"` fns call
// this, with the `lua_State*` Lua passed them; user crates then need no `unsafe`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn entry(l: *mut lua_State, name: &'static str, in_module: bool, body: &dyn Fn(&Lua, c_int) -> Result<Outcome, ArgError>) -> c_int {
    // SAFETY: only called from generated `extern "C-unwind"` fns that Lua invokes with `l`.
    unsafe {
        trampoline::call(l, &|lua| {
            let count = lua.top();
            match body(lua, count) {
                Ok(o) => o,
                Err((n, e)) => bad_argument(lua, &path(lua, name, in_module), n, &e),
            }
        })
    }
}

/// `Ok` value of an export.
pub fn value<R: IntoLuaMulti>(lua: &Lua, entry: c_int, name: &str, in_module: bool, r: R) -> Outcome {
    returns(lua, entry, &path(lua, name, in_module), r)
}

/// Autoref dispatch: `rsgdll::Error` keeps its backtrace, other `Display` errors do not.
pub struct ErrWrap<E>(pub E);

pub trait ViaRsgdll {
    fn lua_text(&self) -> String;
}
impl ViaRsgdll for ErrWrap<Error> {
    fn lua_text(&self) -> String {
        self.0.lua_message()
    }
}

pub trait ViaDisplay {
    fn lua_text(&self) -> String;
}
impl<E: std::fmt::Display> ViaDisplay for &ErrWrap<E> {
    fn lua_text(&self) -> String {
        self.0.to_string()
    }
}

/// Formats an `Err` of an export (`[module] path: message`).
pub fn error(lua: &Lua, name: &str, in_module: bool, text: String) -> Outcome {
    Outcome::Err(trampoline::format_error(lua.module_name(), &path(lua, name, in_module), &text))
}

/// Converts any user error into `rsgdll::Error` (keeps rsgdll errors' formatted text).
pub fn to_error(text: String) -> Error {
    Error::preformatted(text)
}

/// Calls `cb(nil, values...)` or `cb(err_string)`; a raising callback is logged (spec §10.2).
fn deliver<R: IntoLuaMulti>(lua: &Lua, cb: &crate::conv::LuaFunction, path: &str, r: crate::Result<R>) {
    use crate::conv::Stage;
    use crate::protected::Op;
    let g = lua.guard();
    let mut s = Stage::new(lua);
    let staged = s.reference_id(&cb.0).map(|_| ()).and_then(|()| match r {
        Ok(v) => {
            s.nil();
            v.into_lua_multi(&mut s).map(|n| n + 1)
        }
        Err(e) => s.push(trampoline::format_error(lua.module_name(), path, &e.lua_message())).map(|_| 1),
    });
    let res = staged.map_err(Error::from).and_then(|n| {
        s.op(Op::Call(n, 0), -n - 1);
        s.finish().map(|_| ())
    });
    drop(g);
    if let Err(e) = res {
        crate::rt::executor::log_error(lua, &format!("[{}] {path} callback: {}", lua.module_name(), e.lua_message()));
    }
}

/// Runs an async export's future on the state's executor and delivers the result.
pub fn spawn_async<R, F>(lua: &Lua, name: &'static str, in_module: bool, cb: crate::conv::LuaFunction, fut: F)
where
    R: IntoLuaMulti + 'static,
    F: std::future::Future<Output = crate::Result<R>> + 'static,
{
    let local = crate::rt::LocalLua::new(lua);
    crate::rt::executor::spawn_future(lua, async move {
        let r = fut.await;
        if let Ok(l) = local.lua() {
            deliver(l, &cb, &path(l, name, in_module), r);
        }
    });
}

