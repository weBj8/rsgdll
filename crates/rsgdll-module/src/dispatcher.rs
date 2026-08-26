use std::error::Error;
use std::ffi::c_char;
use std::fmt;
use std::sync::RwLock;

use rsgdll_bridge::{DispatchResult, ReturnBuffer};
use rsgdll_lua::{FromLua, Lua, StackFrame};

use crate::{ErrorReport, PanicReport, ReturnWriter};

const ERROR_STATUS: i32 = 1;
const PANIC_STATUS: i32 = 2;
const DISPATCHER_CONTEXT: &str = "rsgdll dispatcher";

pub type BoxError = Box<dyn Error + 'static>;
pub type Callback = for<'guard, 'lua, 'buffer> fn(
    &mut StackFrame<'guard, 'lua>,
    &mut ReturnWriter<'buffer>,
) -> Result<(), BoxError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackId(u32);

impl CallbackId {
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationError;

impl fmt::Display for RegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("callback registry exhausted its u32 identifier space")
    }
}

impl Error for RegistrationError {}

#[derive(Clone, Copy)]
struct Entry {
    context: &'static str,
    callback: Callback,
}

static CALLBACKS: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

pub fn register_callback(
    context: &'static str,
    callback: Callback,
) -> Result<CallbackId, RegistrationError> {
    let mut callbacks = CALLBACKS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let id = u32::try_from(callbacks.len())
        .ok()
        .and_then(|id| id.checked_add(1))
        .ok_or(RegistrationError)?;
    callbacks.push(Entry { context, callback });
    Ok(CallbackId(id))
}

pub fn install_dispatcher() {
    rsgdll_bridge::set_dispatcher(rust_dispatcher);
}

#[must_use]
pub fn trampoline() -> rsgdll_lua::LuaCFunction {
    rsgdll_bridge::trampoline()
}

enum Outcome {
    Success(i32),
    Error(ErrorReport),
    Panic(PanicReport),
}

unsafe extern "C" fn rust_dispatcher(
    state: *mut rsgdll_platform::__private::RawLuaState,
    error_buffer: *mut c_char,
    error_capacity: u32,
    return_buffer: *mut ReturnBuffer,
) -> DispatchResult {
    #[cfg(panic = "unwind")]
    {
        match std::panic::catch_unwind(|| {
            dispatch_and_write(state, error_buffer, error_capacity, return_buffer)
        }) {
            Ok(result) => result,
            Err(_) => {
                let length = write_bytes(
                    error_buffer,
                    error_capacity,
                    b"panic while producing rsgdll panic report",
                );
                DispatchResult::failure(PANIC_STATUS, length)
            }
        }
    }
    #[cfg(not(panic = "unwind"))]
    {
        dispatch_and_write(state, error_buffer, error_capacity, return_buffer)
    }
}

fn dispatch_and_write(
    state: *mut rsgdll_platform::__private::RawLuaState,
    error_buffer: *mut c_char,
    error_capacity: u32,
    return_buffer: *mut ReturnBuffer,
) -> DispatchResult {
    match dispatch(state, return_buffer) {
        Outcome::Success(return_count) => DispatchResult::success(return_count),
        Outcome::Error(report) => {
            let length = write_report(error_buffer, error_capacity, &report);
            drop(report);
            DispatchResult::failure(ERROR_STATUS, length)
        }
        Outcome::Panic(report) => {
            let length = write_report(error_buffer, error_capacity, &report);
            drop(report);
            DispatchResult::failure(PANIC_STATUS, length)
        }
    }
}

fn dispatch(
    state: *mut rsgdll_platform::__private::RawLuaState,
    return_buffer: *mut ReturnBuffer,
) -> Outcome {
    let Some(return_buffer) = (unsafe { return_buffer.as_mut() }) else {
        return Outcome::Error(ErrorReport::message(
            DISPATCHER_CONTEXT,
            "C++ bridge supplied no return buffer",
        ));
    };
    // SAFETY: C++ trampoline passes its live callback state and no throwing Lua
    // operation occurs while constructing the checked handle.
    let mut lua = match unsafe { Lua::from_raw(state) } {
        Ok(lua) => lua,
        Err(error) => {
            return Outcome::Error(ErrorReport::capture(DISPATCHER_CONTEXT, &error));
        }
    };
    let id = match callback_id(&lua) {
        Ok(id) => id,
        Err(report) => return Outcome::Error(report),
    };
    let entry = match callback(id) {
        Some(entry) => entry,
        None => {
            return Outcome::Error(ErrorReport::message(
                DISPATCHER_CONTEXT,
                format!("unknown callback id {}", id.get()),
            ));
        }
    };

    let mut stack = lua.stack();
    let mut frame = stack.frame();
    let mut returns = ReturnWriter::new(return_buffer);
    #[cfg(panic = "unwind")]
    let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (entry.callback)(&mut frame, &mut returns)
            .map_err(|error| ErrorReport::capture(entry.context, error.as_ref()))
    }));
    #[cfg(not(panic = "unwind"))]
    let invocation = Ok((entry.callback)(&mut frame, &mut returns)
        .map_err(|error| ErrorReport::capture(entry.context, error.as_ref())));

    match invocation {
        Ok(Ok(())) => match frame.commit(0) {
            Ok(_) => match i32::try_from(returns.count()) {
                Ok(return_count) => Outcome::Success(return_count),
                Err(_) => Outcome::Error(ErrorReport::message(
                    entry.context,
                    "Lua return count exceeds the ABI integer limit",
                )),
            },
            Err(error) => Outcome::Error(ErrorReport::capture(entry.context, &error)),
        },
        Ok(Err(report)) => match frame.finish() {
            Ok(()) => Outcome::Error(report),
            Err(error) => {
                Outcome::Error(report.append(format_args!("stack restoration failed: {error}")))
            }
        },
        Err(payload) => {
            let report = PanicReport::capture(entry.context, payload);
            match frame.finish() {
                Ok(()) => Outcome::Panic(report),
                Err(error) => {
                    Outcome::Panic(report.append(format_args!("stack restoration failed: {error}")))
                }
            }
        }
    }
}

fn callback_id(lua: &Lua<'_>) -> Result<CallbackId, ErrorReport> {
    let index =
        Lua::upvalue_index(1).map_err(|error| ErrorReport::capture(DISPATCHER_CONTEXT, &error))?;
    let raw = f64::from_lua(lua, index)
        .map_err(|error| ErrorReport::capture(DISPATCHER_CONTEXT, &error))?;
    if !raw.is_finite() || raw.fract() != 0.0 || !(1.0..=f64::from(u32::MAX)).contains(&raw) {
        return Err(ErrorReport::message(
            DISPATCHER_CONTEXT,
            "callback id upvalue is not a valid u32",
        ));
    }
    Ok(CallbackId(raw as u32))
}

fn callback(id: CallbackId) -> Option<Entry> {
    let callbacks = CALLBACKS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = usize::try_from(id.get()).ok()?.checked_sub(1)?;
    callbacks.get(index).copied()
}

fn write_report(buffer: *mut c_char, capacity: u32, report: &impl fmt::Display) -> u32 {
    write_bytes(buffer, capacity, report.to_string().as_bytes())
}

fn write_bytes(buffer: *mut c_char, capacity: u32, message: &[u8]) -> u32 {
    let available = capacity.saturating_sub(1) as usize;
    let length = message.len().min(available);
    if length == 0 || buffer.is_null() {
        return 0;
    }
    // SAFETY: bridge provides writable storage for `capacity` bytes and
    // `length` is strictly smaller than that capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(message.as_ptr(), buffer.cast(), length);
        buffer.add(length).write(0);
    }
    length as u32
}
