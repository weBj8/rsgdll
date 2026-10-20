//! Rust implementation of `ILuaThreadedCall` (spec §5.2, §10.2). `AddThreadedCall` from
//! worker threads is unverified; it is only used when `RSGDLL_THREADED_CALL=1` (set after
//! `cargo xtask probe-threadedcall` passed for the target). Default wake-up = Think hook.

use core::ffi::c_void;

use rsgdll_sys::gmod::ILuaBase;
use rsgdll_sys::vfn;
use rsgdll_sys::vtables::ILuaThreadedCallVTable;

use crate::error::{Error, Result};
use crate::lua::Lua;

/// One heap-allocated call; GMod calls `Init` then `Run(ILuaBase*)` on the main thread.
#[repr(C)]
struct Call {
    vtable: *const ILuaThreadedCallVTable,
    run: Box<dyn FnOnce(*mut ILuaBase) + Send>,
}

#[cfg(not(all(windows, target_arch = "x86")))]
mod abi {
    use super::*;
    pub unsafe extern "C-unwind" fn init(_this: *mut c_void) {}
    pub unsafe extern "C-unwind" fn run(this: *mut c_void, base: *mut c_void) {
        unsafe { super::run_impl(this, base) }
    }
}

#[cfg(all(windows, target_arch = "x86"))]
mod abi {
    use super::*;
    pub unsafe extern "thiscall-unwind" fn init(_this: *mut c_void) {}
    pub unsafe extern "thiscall-unwind" fn run(this: *mut c_void, base: *mut c_void) {
        unsafe { super::run_impl(this, base) }
    }
}

unsafe fn run_impl(this: *mut c_void, base: *mut c_void) {
    // SAFETY: `this` is the Box leaked by `add`; GMod calls Run exactly once.
    let call = unsafe { Box::from_raw(this.cast::<Call>()) };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || (call.run)(base.cast())));
    if r.is_err() {
        rsgdll_sys::tier0::warning("rsgdll: threaded call panicked\n");
    }
}

static VTABLE: ILuaThreadedCallVTable = ILuaThreadedCallVTable { init: abi::init as vfn!((*mut c_void)), run: abi::run as vfn!((*mut c_void, *mut c_void)) };

/// Whether threaded-call wake-ups are enabled (probe-gated).
pub fn enabled() -> bool {
    std::env::var("RSGDLL_THREADED_CALL").is_ok_and(|v| v == "1")
}

/// Queues `f` to run on the main thread via `ILuaInterface::AddThreadedCall`.
pub fn add(lua: &Lua, f: impl FnOnce(*mut ILuaBase) + Send + 'static) -> Result<()> {
    let iface = lua.interface()?.raw();
    let call = Box::into_raw(Box::new(Call { vtable: &VTABLE, run: Box::new(f) }));
    // SAFETY: AddThreadedCall takes ownership semantics of the pointer until Run.
    let _ = unsafe { ((*iface).vt().add_threaded_call)(iface.cast(), call.cast()) };
    Ok(())
}

/// Same, from an arbitrary thread with a raw interface pointer (probe/test use only).
///
/// # Safety
/// `iface` must be a live `ILuaInterface*`, and calling it from this thread must be safe
/// (that is what `probe-threadedcall` verifies).
pub unsafe fn add_raw(iface: usize, f: impl FnOnce(*mut ILuaBase) + Send + 'static) -> Result<()> {
    let iface = iface as *mut rsgdll_sys::gmod::ILuaInterface;
    if iface.is_null() {
        return Err(Error::msg("null ILuaInterface"));
    }
    let call = Box::into_raw(Box::new(Call { vtable: &VTABLE, run: Box::new(f) }));
    let _ = unsafe { ((*iface).vt().add_threaded_call)(iface.cast(), call.cast()) };
    Ok(())
}
