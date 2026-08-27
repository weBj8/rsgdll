use std::error::Error;
use std::ffi::c_char;
use std::fmt;
use std::sync::RwLock;

use rsgdll_bridge::{
    DispatchResult, ReturnBuffer, STATUS_INTERNAL_ERROR, STATUS_RUST_ERROR, STATUS_RUST_PANIC,
};
use rsgdll_lua::{FromLua, Lua, StackFrame};

use crate::{ErrorReport, PanicReport, ReturnWriter};

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
    if let Some(index) = callbacks.iter().position(|entry| {
        entry.context == context && std::ptr::fn_addr_eq(entry.callback, callback)
    }) {
        let id = u32::try_from(index)
            .ok()
            .and_then(|id| id.checked_add(1))
            .ok_or(RegistrationError)?;
        return Ok(CallbackId(id));
    }
    let id = u32::try_from(callbacks.len())
        .ok()
        .and_then(|id| id.checked_add(1))
        .ok_or(RegistrationError)?;
    callbacks.push(Entry { context, callback });
    Ok(CallbackId(id))
}

pub fn install_dispatcher() {
    // SAFETY: `rust_dispatcher` catches panics, borrows every pointer only for
    // this call, and constructs results from validated stack/staging counts.
    unsafe { rsgdll_bridge::set_dispatcher(rust_dispatcher) };
}

#[must_use]
pub fn trampoline() -> rsgdll_lua::LuaCFunction {
    rsgdll_bridge::trampoline()
}

enum Outcome {
    Success(DispatchResult),
    ApplicationError(ErrorReport),
    InternalError(ErrorReport),
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
            Err(_) => DispatchResult::failure(
                STATUS_RUST_PANIC,
                write_bytes(
                    error_buffer,
                    error_capacity,
                    b"panic while producing rsgdll panic report",
                ),
            ),
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
    let error_result = |report: ErrorReport, status| {
        DispatchResult::failure(status, write_report(error_buffer, error_capacity, &report))
    };
    match dispatch(state, return_buffer) {
        Outcome::Success(result) => result,
        Outcome::ApplicationError(report) => error_result(report, STATUS_RUST_ERROR),
        Outcome::InternalError(report) => error_result(report, STATUS_INTERNAL_ERROR),
        Outcome::Panic(report) => DispatchResult::failure(
            STATUS_RUST_PANIC,
            write_report(error_buffer, error_capacity, &report),
        ),
    }
}

fn dispatch(
    state: *mut rsgdll_platform::__private::RawLuaState,
    return_buffer: *mut ReturnBuffer,
) -> Outcome {
    let Some(return_buffer) = (unsafe { return_buffer.as_mut() }) else {
        return Outcome::InternalError(ErrorReport::message(
            DISPATCHER_CONTEXT,
            "C++ bridge supplied no return buffer",
        ));
    };
    // SAFETY: C++ trampoline passes its live callback state and no throwing Lua
    // operation occurs while constructing the checked handle.
    let mut lua = match unsafe { rsgdll_lua::__private::from_raw(state) } {
        Ok(lua) => lua,
        Err(error) => {
            return Outcome::InternalError(ErrorReport::capture(DISPATCHER_CONTEXT, &error));
        }
    };
    let id = match callback_id(&lua) {
        Ok(id) => id,
        Err(report) => return Outcome::InternalError(report),
    };
    let entry = match callback(id) {
        Some(entry) => entry,
        None => {
            return Outcome::InternalError(ErrorReport::message(
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
        Ok(Ok(())) => {
            let stack_count = returns.stack_count();
            let expected = stack_count.unwrap_or(0);
            match frame.commit(expected) {
                Ok(_) => match i32::try_from(stack_count.unwrap_or_else(|| returns.count())) {
                    Ok(return_count) => Outcome::Success(if stack_count.is_some() {
                        DispatchResult::stack_success(return_count)
                    } else {
                        DispatchResult::success(return_count)
                    }),
                    Err(_) => Outcome::InternalError(ErrorReport::message(
                        entry.context,
                        "Lua return count exceeds the ABI integer limit",
                    )),
                },
                Err(error) => Outcome::InternalError(ErrorReport::capture(entry.context, &error)),
            }
        }
        Ok(Err(report)) => match frame.finish() {
            Ok(()) => Outcome::ApplicationError(report),
            Err(error) => Outcome::InternalError(
                report.append(format_args!("stack restoration failed: {error}")),
            ),
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

pub(crate) fn write_report(buffer: *mut c_char, capacity: u32, report: &impl fmt::Display) -> u32 {
    let message = report.to_string().replace('\0', "\\0");
    write_bytes(buffer, capacity, message.as_bytes())
}

pub(crate) fn write_bytes(buffer: *mut c_char, capacity: u32, message: &[u8]) -> u32 {
    const TRUNCATION_MARKER: &[u8] = b"\n...[truncated]";

    let available = capacity.saturating_sub(1) as usize;
    let (message_length, suffix) =
        if message.len() > available && available >= TRUNCATION_MARKER.len() {
            (available - TRUNCATION_MARKER.len(), TRUNCATION_MARKER)
        } else {
            (message.len().min(available), &[][..])
        };
    let length = message_length + suffix.len();
    if length == 0 || buffer.is_null() {
        return 0;
    }
    // SAFETY: bridge provides writable storage for `capacity` bytes and
    // `length` is strictly smaller than that capacity.
    unsafe {
        for (index, byte) in message[..message_length].iter().copied().enumerate() {
            buffer
                .cast::<u8>()
                .add(index)
                .write(if byte == 0 { b'?' } else { byte });
        }
        std::ptr::copy_nonoverlapping(
            suffix.as_ptr(),
            buffer.cast::<u8>().add(message_length),
            suffix.len(),
        );
        buffer.add(length).write(0);
    }
    length as u32
}

#[cfg(test)]
mod tests {
    use super::write_bytes;

    #[test]
    fn oversized_reports_end_with_truncation_marker() {
        let mut buffer = [0_i8; 24];

        let length = write_bytes(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            b"abcdefghijklmnopqrstuvwxyz",
        );

        let bytes = buffer[..length as usize]
            .iter()
            .map(|byte| *byte as u8)
            .collect::<Vec<_>>();
        assert_eq!(bytes, b"abcdefgh\n...[truncated]");
        assert_eq!(buffer[length as usize], 0);
    }

    #[test]
    fn interior_nul_is_replaced_before_crossing_c_string_boundary() {
        let mut buffer = [0_i8; 16];

        let length = write_bytes(buffer.as_mut_ptr(), buffer.len() as u32, b"outer\0source");

        let bytes = buffer[..length as usize]
            .iter()
            .map(|byte| *byte as u8)
            .collect::<Vec<_>>();
        assert_eq!(bytes, b"outer?source");
        assert_eq!(buffer[length as usize], 0);
    }
}
