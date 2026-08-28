mod close;

use rsgdll::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

static COUNTER_DROPS: AtomicU64 = AtomicU64::new(0);
static BACKGROUND: OnceLock<BackgroundCompletion> = OnceLock::new();
static DEBUG_LINE_EVENTS: AtomicU64 = AtomicU64::new(0);
static DEBUG_LOCAL: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());
static DEBUG_UPVALUE: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());

thread_local! {
    static DEBUG_HOOK: RefCell<Option<DebugHookGuard>> = const { RefCell::new(None) };
}

#[rsgdll::module(close = close::run)]
fn module(module: &mut ModuleBuilder) {
    module
        .function("plain", plain)
        .function("add", add)
        .function("unsigned_integer_round_trip", unsigned_integer_round_trip)
        .function("signed_integer_round_trip", signed_integer_round_trip)
        .function("primitives", primitives)
        .function("result_ok", result_ok)
        .function("result_err", result_err)
        .function("overflow_stack", overflow_stack)
        .function("panic_now", panic_now)
        .function("make_table", make_table)
        .function("recover_table_set", recover_table_set)
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
        .function("serde_round_trip", serde_round_trip)
        .function("start_background", start_background)
        .function("complete_background", complete_background)
        .function("debug_attach", debug_attach)
        .function("debug_detach", debug_detach)
        .function("debug_observation", debug_observation);
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    module.function("engine_is_dedicated", engine_is_dedicated);
    #[cfg(feature = "crash-test")]
    module.function("native_crash", native_crash);
}

#[rsgdll::function]
fn plain() -> &'static str {
    "plain Rust call"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[rsgdll::function]
fn engine_is_dedicated(main_thread: &mut MainThread) -> Result<bool, rsgdll::engine::EngineError> {
    let engine = rsgdll::engine::Engine::attach(main_thread)?;
    Ok(engine.server()?.is_dedicated_server())
}

#[rsgdll::function]
fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[rsgdll::function]
fn unsigned_integer_round_trip(
    u8_value: u8,
    u16_value: u16,
    u32_value: u32,
    u64_value: u64,
) -> (u8, u16, u32, u64) {
    (u8_value, u16_value, u32_value, u64_value)
}

#[rsgdll::function]
fn signed_integer_round_trip(
    i8_value: i8,
    i16_value: i16,
    i32_value: i32,
    i64_value: i64,
) -> (i8, i16, i32, i64) {
    (i8_value, i16_value, i32_value, i64_value)
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
fn overflow_stack(frame: &mut StackFrame<'_, '_>) -> Result<(), LuaError> {
    for _ in 0..130 {
        frame.push(())?;
    }
    Ok(())
}

#[rsgdll::function]
fn panic_now() {
    panic!("intentional E2E panic");
}

#[cfg(feature = "crash-test")]
#[rsgdll::function]
fn native_crash() {
    std::process::abort();
}

#[rsgdll::function]
fn make_table(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    frame.create_table()?;
    frame.push("answer")?;
    frame.push(42.0)?;
    frame.raw_set(-3)?;
    frame.push("label")?;
    frame.push("from Rust")?;
    frame.raw_set(-3)?;
    Ok(LuaStackValues::new(1))
}

struct PushThenFail;

impl IntoLua for PushThenFail {
    fn into_lua(self, lua: &mut Lua<'_>) -> Result<(), LuaError> {
        true.into_lua(lua)?;
        Err(LuaError::CountOverflow)
    }
}

#[rsgdll::function]
fn recover_table_set(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    let table = frame.new_table()?;
    match table.set(frame, "key", PushThenFail) {
        Err(LuaError::CountOverflow) => Ok(42.0),
        Err(error) => Err(error),
        Ok(()) => Err(LuaError::CountOverflow),
    }
}

#[rsgdll::function]
fn table_answer(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    frame.push("answer")?;
    frame.raw_get(1)?;
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
    plus_one
        .push(frame)
        .map_err(|error| E2eSurfaceError(error.to_string()))?;
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn call_once(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    let function = frame.function(1)?;
    let argument = frame.get::<f64>(2)?;
    let (result,) = function.call::<_, (f64,)>(frame, (argument,))?;
    Ok(result)
}

#[rsgdll::function]
fn call_multi(frame: &mut StackFrame<'_, '_>) -> Result<(String, f64, bool), LuaError> {
    let function = frame.function(1)?;
    function.call(frame, ())
}

#[rsgdll::function]
fn table_and_value(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    frame.create_table()?;
    frame.push("kind")?;
    frame.push("complex")?;
    frame.raw_set(-3)?;
    frame.push(9.0)?;
    Ok(LuaStackValues::new(2))
}

#[rsgdll::function]
fn registry_roundtrip(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, LuaError> {
    let reference = frame.create_reference(1)?;
    reference.push(frame)?;
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
    let kind = frame
        .userdata_type::<Counter>("rsgdll_e2e.Counter")
        .map_err(E2eSurfaceError::from)?;
    counter_add
        .install_method(frame, &kind, "add")
        .map_err(|error| E2eSurfaceError(error.to_string()))?;
    counter_value
        .install_method(frame, &kind, "value")
        .map_err(|error| E2eSurfaceError(error.to_string()))?;
    kind.push(frame, Counter { value: initial })
        .map_err(E2eSurfaceError::from)?;
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn counter_add(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    let kind = frame.userdata_type::<Counter>("rsgdll_e2e.Counter")?;
    let amount = frame.get::<f64>(2)?;
    let mut counter = kind.borrow_mut(frame, 1)?;
    counter.value += amount;
    Ok(counter.value)
}

#[rsgdll::function]
fn counter_value(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    let kind = frame.userdata_type::<Counter>("rsgdll_e2e.Counter")?;
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

#[rsgdll::function(serde)]
fn serde_round_trip(value: SerdeConfig) -> Result<SerdeConfig, LuaError> {
    Ok(value)
}

struct BackgroundCompletion {
    sender: CompletionSender<u64>,
    queue: Mutex<CompletionQueue<u64>>,
}

fn background() -> &'static BackgroundCompletion {
    BACKGROUND.get_or_init(|| {
        let (sender, queue) = completion_queue(NonZeroUsize::MIN);
        BackgroundCompletion {
            sender,
            queue: Mutex::new(queue),
        }
    })
}

#[rsgdll::function]
fn start_background(value: u64) -> Result<(), E2eSurfaceError> {
    let sender = background().sender.clone();
    let worker = std::thread::spawn(move || sender.send(value + 1));
    worker
        .join()
        .map_err(|_| E2eSurfaceError("background worker panicked".to_owned()))?
        .map_err(|_| E2eSurfaceError("completion queue closed".to_owned()))
}

#[rsgdll::function]
fn complete_background(
    main_thread: &mut MainThread,
    frame: &mut StackFrame<'_, '_>,
) -> Result<f64, E2eSurfaceError> {
    let callback = frame.function(1)?;
    let mut completed = None;
    background()
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(main_thread, |_, value| completed = Some(value));
    let value = completed
        .ok_or_else(|| E2eSurfaceError("no background completion queued".to_owned()))?;
    let (returned,) = callback.call::<_, (f64,)>(frame, (value as f64,))?;
    Ok(returned)
}

#[rsgdll::function]
fn debug_attach(frame: &mut StackFrame<'_, '_>) -> Result<bool, LuaError> {
    DEBUG_LINE_EVENTS.store(0, Ordering::Relaxed);
    DEBUG_LOCAL.store(f64::NAN.to_bits(), Ordering::Relaxed);
    DEBUG_UPVALUE.store(f64::NAN.to_bits(), Ordering::Relaxed);
    DEBUG_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook.is_some() {
            return Ok(false);
        }
        *hook = Some(frame.install_debug_hook(DebugMask::LINES, 0, inspect_debug_event)?);
        Ok(true)
    })
}

#[rsgdll::function]
fn debug_detach(frame: &mut StackFrame<'_, '_>) -> Result<bool, LuaError> {
    DEBUG_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        let Some(mut guard) = hook.take() else {
            return Ok(false);
        };
        guard.restore_with_frame(frame)?;
        Ok(true)
    })
}

#[rsgdll::function]
fn debug_observation() -> (u64, f64, f64) {
    (
        DEBUG_LINE_EVENTS.load(Ordering::Relaxed),
        f64::from_bits(DEBUG_LOCAL.load(Ordering::Relaxed)),
        f64::from_bits(DEBUG_UPVALUE.load(Ordering::Relaxed)),
    )
}

fn inspect_debug_event(mut context: DebugContext<'_>) {
    if context.event() != DebugEvent::Line {
        return;
    }
    DEBUG_LINE_EVENTS.fetch_add(1, Ordering::Relaxed);
    let mut frame = context.current_frame();
    for index in 1..=64 {
        let Ok(Some(local)) = frame.local(index) else {
            break;
        };
        if local.name().as_bytes() == b"rsgdebug_local_probe"
            && let Ok(value) = local.get::<f64>()
        {
            DEBUG_LOCAL.store(value.to_bits(), Ordering::Relaxed);
        }
    }
    for index in 1..=64 {
        let Ok(Some(upvalue)) = frame.upvalue(index) else {
            break;
        };
        if upvalue.name().as_bytes() == b"rsgdebug_upvalue_probe"
            && let Ok(value) = upvalue.get::<f64>()
        {
            DEBUG_UPVALUE.store(value.to_bits(), Ordering::Relaxed);
        }
    }
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
