mod support;

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use rsgdll_module::{BoxError, install_dispatcher, register_callback, trampoline};
use support::{Fixture, Value};

static ERROR_DROPPED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct ArtificialError;

impl fmt::Display for ArtificialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("artificial failure")
    }
}

impl Error for ArtificialError {}

impl Drop for ArtificialError {
    fn drop(&mut self) {
        ERROR_DROPPED.store(true, Ordering::SeqCst);
    }
}

fn returns_error(frame: &mut rsgdll_lua::StackFrame<'_, '_>) -> Result<usize, BoxError> {
    // SAFETY: test fixture's push cannot allocate or longjmp.
    unsafe { frame.push(41.0_f64)? };
    Err(Box::new(ArtificialError))
}

fn panics(_: &mut rsgdll_lua::StackFrame<'_, '_>) -> Result<usize, BoxError> {
    panic!("artificial panic")
}

fn succeeds(frame: &mut rsgdll_lua::StackFrame<'_, '_>) -> Result<usize, BoxError> {
    // SAFETY: test fixture's push cannot allocate or longjmp.
    unsafe { frame.push(42.0_f64)? };
    Ok(1)
}

fn wrong_return_count(frame: &mut rsgdll_lua::StackFrame<'_, '_>) -> Result<usize, BoxError> {
    // SAFETY: test fixture's push cannot allocate or longjmp.
    unsafe { frame.push(42.0_f64)? };
    Ok(2)
}

#[test]
fn rust_error_returns_before_cpp_throws_and_restores_stack() {
    ERROR_DROPPED.store(false, Ordering::SeqCst);
    install_dispatcher();
    let id = register_callback("module.test_error", returns_error).expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![Value::Number(7.0)]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let returned = unsafe { trampoline()(fixture.state()) };

    assert_eq!(returned, 0);
    assert!(ERROR_DROPPED.load(Ordering::SeqCst));
    assert_eq!(fixture.stack(), &[Value::Number(7.0)]);
    assert_eq!(
        fixture.error(),
        Some("module.test_error: artificial failure")
    );
}

#[cfg(panic = "unwind")]
#[test]
fn rust_panic_returns_before_cpp_throws_and_restores_stack() {
    install_dispatcher();
    let id = register_callback("module.test_panic", panics).expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![Value::Number(7.0)]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let returned = unsafe { trampoline()(fixture.state()) };

    assert_eq!(returned, 0);
    assert_eq!(fixture.stack(), &[Value::Number(7.0)]);
    assert_eq!(
        fixture.error(),
        Some("panic in module.test_panic: artificial panic")
    );
}

#[test]
fn success_preserves_return_values_without_throwing() {
    install_dispatcher();
    let id = register_callback("module.test_success", succeeds).expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let returned = unsafe { trampoline()(fixture.state()) };

    assert_eq!(returned, 1);
    assert_eq!(fixture.stack(), &[Value::Number(42.0)]);
    assert_eq!(fixture.error(), None);
}

#[test]
fn mismatched_return_count_is_reported_and_restored() {
    install_dispatcher();
    let id =
        register_callback("module.bad_count", wrong_return_count).expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let returned = unsafe { trampoline()(fixture.state()) };

    assert_eq!(returned, 0);
    assert_eq!(fixture.stack(), &[]);
    assert_eq!(
        fixture.error(),
        Some("module.bad_count: callback declared 2 return values but left 1 on the stack")
    );
}
