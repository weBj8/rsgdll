//! Export trampolines (spec §7.3) and panic hook.
//!
//! Generated code calls [`call`] from an `extern "C-unwind"` fn. All Rust work happens in a
//! non-inlined `run` under `catch_unwind`; by the time `lua_error` runs only a `c_int`/`bool`
//! is live in the trampoline frame.

use core::ffi::c_int;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use rsgdll_sys::lua::lua_State;

use crate::error::Error;
use crate::lua::Lua;
use crate::protected::Op;

thread_local! {
    static IN_TRAMPOLINE: RefCell<u32> = const { RefCell::new(0) };
    static LAST_PANIC: RefCell<Option<(String, Option<String>)>> = const { RefCell::new(None) };
}

/// Installs the per-DLL panic hook; chains to the previous hook outside trampolines.
pub fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let inside = IN_TRAMPOLINE.with(|d| *d.borrow() > 0);
            if inside {
                let msg = info.to_string();
                let bt = crate::error::backtrace_enabled()
                    .then(|| crate::error::trim_backtrace(&std::backtrace::Backtrace::force_capture().to_string()));
                LAST_PANIC.with(|p| *p.borrow_mut() = Some((msg, bt)));
            } else {
                prev(info);
            }
        }));
    });
}

fn panic_message() -> String {
    let (msg, bt) = LAST_PANIC.with(|p| p.borrow_mut().take()).unwrap_or_else(|| ("unknown panic".into(), None));
    let msg = msg.strip_prefix("panicked at ").map_or(msg.clone(), |m| m.to_owned());
    match bt {
        Some(bt) if !bt.is_empty() => format!("rust panic: {msg}\nrust stack traceback:\n{bt}"),
        _ => format!("rust panic: {msg}"),
    }
}

/// Outcome of the Rust part of an export.
pub enum Outcome {
    /// Number of values pushed.
    Ok(c_int),
    /// Error message (already formatted for Lua).
    Err(String),
}

/// Formats an error for Lua (`[module] path: message`, spec §7.2).
pub fn format_error(module: &str, path: &str, message: &str) -> String {
    format!("[{module}] {path}: {message}")
}

/// Pushes `msg` (protected); falls back to the pre-interned OOM message.
pub fn push_message(lua: &Lua, msg: &str) {
    let base = lua.top();
    if lua.run(&[], &[Op::PushStr(msg.as_ptr(), msg.len())]).is_err() {
        // SAFETY: shrinking + rawgeti never allocate.
        unsafe { (lua.api().lua_settop)(lua.raw(), base) };
        lua.push_ref(lua.runtime().boot.oom_message);
    }
}

#[inline(never)]
fn run(l: *mut lua_State, f: &dyn Fn(&Lua) -> Outcome) -> Option<c_int> {
    IN_TRAMPOLINE.with(|d| *d.borrow_mut() += 1);
    let r = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: called by Lua with a live state.
        match unsafe { Lua::from_raw(l) } {
            Ok(lua) => {
                let entry = lua.top();
                match f(&lua) {
                    Outcome::Ok(n) => Some(n),
                    Outcome::Err(msg) => {
                        // SAFETY: restore to entry_top before pushing the message.
                        unsafe { (lua.api().lua_settop)(l, entry) };
                        push_message(&lua, &msg);
                        None
                    }
                }
            }
            Err(_) => {
                let msg = "rsgdll: module function called after its state was closed";
                // SAFETY: no runtime: push via the raw API. pushstring may raise on OOM; there is
                // no Drop value in this frame, so that is acceptable.
                unsafe { (rsgdll_sys::api().lua_pushlstring)(l, msg.as_ptr().cast(), msg.len()) };
                None
            }
        }
    }));
    IN_TRAMPOLINE.with(|d| *d.borrow_mut() -= 1);
    match r {
        Ok(v) => v,
        Err(payload) => {
            drop(payload);
            let msg = panic_message();
            // SAFETY: as above.
            match unsafe { Lua::from_raw(l) } {
                Ok(lua) => push_message(&lua, &msg),
                Err(_) => unsafe { (rsgdll_sys::api().lua_pushlstring)(l, msg.as_ptr().cast(), msg.len()) },
            }
            None
        }
    }
}

/// Body of every generated trampoline. Raises via `lua_error` with nothing `Drop` alive.
///
/// # Safety
/// Must be called directly from an `extern "C-unwind"` fn invoked by Lua.
#[inline(always)]
pub unsafe fn call(l: *mut lua_State, f: &dyn Fn(&Lua) -> Outcome) -> c_int {
    let nret: Option<c_int> = run(l, f);
    match nret {
        Some(n) => n,
        // SAFETY: the message is on top; only plain values live in this frame.
        None => unsafe { (rsgdll_sys::api().lua_error)(l) },
    }
}

/// Converts any `E: Display` user error into a Lua message; `rsgdll::Error` keeps its backtrace.
pub trait IntoLuaMessage {
    fn into_lua_message(self) -> String;
}

impl IntoLuaMessage for Error {
    fn into_lua_message(self) -> String {
        self.lua_message()
    }
}

/// Plain `Display` errors: no backtrace (it would only show trampoline frames).
pub struct Displayed<E>(pub E);

impl<E: std::fmt::Display> IntoLuaMessage for Displayed<E> {
    fn into_lua_message(self) -> String {
        self.0.to_string()
    }
}
