use std::cell::RefCell;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use rsgdll::prelude::*;

thread_local! {
    static HOOK: RefCell<Option<DebugHookGuard>> = const { RefCell::new(None) };
}

static LINE_EVENTS: AtomicU64 = AtomicU64::new(0);
static LAST_LINE: AtomicI32 = AtomicI32::new(-1);

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
    module
        .function("attach", attach)
        .function("detach", detach)
        .function("line_events", line_events)
        .function("last_line", last_line);
}

#[rsgdll::function]
fn attach(frame: &mut StackFrame<'_, '_>) -> Result<bool, LuaError> {
    HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook.is_some() {
            return Ok(false);
        }
        *hook = Some(frame.install_debug_hook(DebugMask::LINES, 0, on_debug_event)?);
        Ok(true)
    })
}

#[rsgdll::function]
fn detach(frame: &mut StackFrame<'_, '_>) -> Result<bool, LuaError> {
    HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        let Some(mut guard) = hook.take() else {
            return Ok(false);
        };
        guard.restore_with_frame(frame)?;
        Ok(true)
    })
}

#[rsgdll::function]
fn line_events() -> u64 {
    LINE_EVENTS.load(Ordering::Relaxed)
}

#[rsgdll::function]
fn last_line() -> i32 {
    LAST_LINE.load(Ordering::Relaxed)
}

fn on_debug_event(mut context: DebugContext<'_>) {
    if context.event() != DebugEvent::Line {
        return;
    }
    LINE_EVENTS.fetch_add(1, Ordering::Relaxed);
    if let Ok(info) = context.current_frame().info() {
        LAST_LINE.store(info.current_line, Ordering::Relaxed);
    }
}
