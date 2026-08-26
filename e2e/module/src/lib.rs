use rsgdll::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

static COUNTER_DROPS: AtomicU64 = AtomicU64::new(0);

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
    module
        .function("plain", plain)
        .function("add", add)
        .function("primitives", primitives)
        .function("result_ok", result_ok)
        .function("result_err", result_err)
        .function("panic_now", panic_now)
        .function("make_table", make_table)
        .function("table_answer", table_answer)
        .function("export_plus_one", export_plus_one)
        .function("call_once", call_once)
        .function("call_multi", call_multi)
        .function("table_and_value", table_and_value)
        .function("registry_roundtrip", registry_roundtrip)
        .function("new_counter", new_counter)
        .function("counter_add", counter_add)
        .function("counter_value", counter_value)
        .function("counter_drops", counter_drops)
        .function("binary_echo", binary_echo)
        .function("serde_round_trip", serde_round_trip);
}

#[rsgdll::function]
fn plain() -> &'static str {
    "plain Rust call"
}

#[rsgdll::function]
fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[rsgdll::function]
fn primitives() -> (&'static str, u64, bool) {
    ("converted", 7, true)
}

#[rsgdll::function]
fn result_ok() -> Result<&'static str, E2eError> {
    Ok("ok")
}

#[rsgdll::function]
fn result_err() -> Result<(), E2eError> {
    Err(E2eError { source: InnerError })
}

#[rsgdll::function]
fn panic_now() {
    panic!("intentional E2E panic");
}

#[rsgdll::function]
fn make_table(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    // SAFETY: exercised inside GMod's callback boundary; these ordinary pushes
    // and table assignments are not allowed to longjmp across Rust.
    unsafe {
        frame.create_table();
        frame.push("answer")?;
        frame.push(42.0)?;
        frame.raw_set(-3)?;
        frame.push("label")?;
        frame.push("from Rust")?;
        frame.raw_set(-3)?;
    }
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn table_answer(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    // SAFETY: argument one is validated as a table by raw_get.
    unsafe {
        frame.push("answer")?;
        frame.raw_get(1)?;
    }
    let answer = frame.get(-1)?;
    frame.pop(1)?;
    Ok(answer)
}

#[rsgdll::function]
fn plus_one(value: f64) -> f64 {
    value + 1.0
}

#[rsgdll::function]
fn export_plus_one(
    frame: &mut StackFrame<'_, '_>,
) -> Result<LuaStackValues, E2eSurfaceError> {
    // SAFETY: generated closure push runs inside the module callback boundary.
    unsafe {
        plus_one
            .push(frame)
            .map_err(|error| E2eSurfaceError(error.to_string()))?
    };
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn call_once(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    // SAFETY: registry operations and argument push run inside callback
    // boundary; invocation itself uses ILuaBase::PCall.
    let function = unsafe { frame.function(1)? };
    let argument = frame.get::<f64>(2)?;
    let (result,) = unsafe { function.call::<_, (f64,)>(frame, (argument,))? };
    Ok(result)
}

#[rsgdll::function]
fn call_multi(frame: &mut StackFrame<'_, '_>) -> Result<(String, f64, bool), LuaError> {
    // SAFETY: registry operations run inside callback boundary; invocation is
    // protected by ILuaBase::PCall.
    let function = unsafe { frame.function(1)? };
    unsafe { function.call(frame, ()) }
}

#[rsgdll::function]
fn table_and_value(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    // SAFETY: ordinary table/value pushes run inside callback boundary.
    unsafe {
        frame.create_table();
        frame.push("kind")?;
        frame.push("complex")?;
        frame.raw_set(-3)?;
        frame.push(9.0)?;
    }
    Ok(LuaStackValues::new(2))
}

#[rsgdll::function]
fn registry_roundtrip(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    // SAFETY: registry create/push run inside callback boundary.
    let reference = unsafe { frame.create_reference(1)? };
    unsafe { reference.push(frame)? };
    Ok(LuaStackValues::new(1))
}

struct Counter {
    value: f64,
}

impl Drop for Counter {
    fn drop(&mut self) {
        COUNTER_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

#[rsgdll::function]
fn new_counter(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, E2eSurfaceError> {
    let initial = frame.get::<f64>(1)?;
    // SAFETY: userdata/metatable/closure operations run inside callback
    // boundary and are covered by the public unsafe contracts.
    unsafe {
        let kind = frame
            .userdata_type::<Counter>("rsgdll_e2e.Counter")
            .map_err(E2eSurfaceError::from)?;
        counter_add
            .install_method(frame, &kind, "add")
            .map_err(|error| E2eSurfaceError(error.to_string()))?;
        counter_value
            .install_method(frame, &kind, "value")
            .map_err(|error| E2eSurfaceError(error.to_string()))?;
        install_userdata_gc(frame, &kind)
            .map_err(|error| E2eSurfaceError(error.to_string()))?;
        kind.push(frame, Counter { value: initial })
            .map_err(E2eSurfaceError::from)?;
    }
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn counter_add(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    // SAFETY: retrieving an existing named metatable runs inside callback.
    let kind = unsafe { frame.userdata_type::<Counter>("rsgdll_e2e.Counter")? };
    let amount = frame.get::<f64>(2)?;
    let mut counter = kind.borrow_mut(frame, 1)?;
    counter.value += amount;
    Ok(counter.value)
}

#[rsgdll::function]
fn counter_value(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    // SAFETY: retrieving an existing named metatable runs inside callback.
    let kind = unsafe { frame.userdata_type::<Counter>("rsgdll_e2e.Counter")? };
    let value = kind.borrow(frame, 1)?.value;
    Ok(value)
}

#[rsgdll::function]
fn counter_drops() -> u64 {
    COUNTER_DROPS.load(Ordering::Relaxed)
}

#[rsgdll::function]
fn binary_echo(value: LuaBytes) -> LuaBytes {
    value
}

#[derive(Serialize, Deserialize)]
struct SerdeConfig {
    name: String,
    enabled: bool,
    scores: Vec<u64>,
}

#[rsgdll::function]
fn serde_round_trip(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    // SAFETY: table iteration and serialization pushes run inside callback.
    let value: SerdeConfig = unsafe { rsgdll::lua::serde::from_lua(frame, 1)? };
    unsafe { rsgdll::lua::serde::to_lua(frame, &value)? };
    Ok(LuaStackValues::new(1))
}

#[derive(Debug, Error)]
#[error("outer E2E failure")]
struct E2eError {
    #[source]
    source: InnerError,
}

#[derive(Debug, Error)]
#[error("inner E2E cause")]
struct InnerError;

#[derive(Debug, Error)]
#[error("{0}")]
struct E2eSurfaceError(String);

impl From<LuaError> for E2eSurfaceError {
    fn from(error: LuaError) -> Self {
        Self(error.to_string())
    }
}
