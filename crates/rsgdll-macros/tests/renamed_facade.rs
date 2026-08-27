use framework::prelude::{MainThread, ModuleBuilder};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

static CLOSED: AtomicBool = AtomicBool::new(false);
static PANIC_ON_CLOSE: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize, Serialize)]
struct Config {
    enabled: bool,
}

#[framework::function]
fn echo(_main_thread: &mut MainThread, input: String) -> String {
    input
}

#[framework::function(serde)]
fn serde_round_trip(input: Config) -> Config {
    input
}

#[framework::module(close = on_close)]
fn register(module: &mut ModuleBuilder) {
    module
        .function("echo", echo)
        .function("serde_round_trip", serde_round_trip);
}

fn on_close() {
    CLOSED.store(true, Ordering::Relaxed);
    if PANIC_ON_CLOSE.swap(false, Ordering::Relaxed) {
        panic!("close hook panic");
    }
}

#[test]
fn generated_descriptor_is_available() {
    let _descriptor = echo;
    let _serde_descriptor = serde_round_trip;
}

#[test]
fn close_hook_runs_when_gmod13_close_is_called() {
    CLOSED.store(false, Ordering::Relaxed);

    // SAFETY: null state is accepted by the teardown-only close entrypoint.
    let result = unsafe { gmod13_close(std::ptr::null_mut()) };

    assert_eq!(result, 0);
    assert!(CLOSED.load(Ordering::Relaxed));

    PANIC_ON_CLOSE.store(true, Ordering::Relaxed);

    // SAFETY: null state is accepted by the teardown-only close entrypoint.
    let panic_result = unsafe { gmod13_close(std::ptr::null_mut()) };

    assert_eq!(panic_result, 0);
}
