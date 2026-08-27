mod support;

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use rsgdll_bridge::{STATUS_INTERNAL_ERROR, STATUS_RUST_ERROR, STATUS_RUST_PANIC};
use rsgdll_module::{BoxError, ReturnWriter, install_dispatcher, register_callback, trampoline};
use support::{Fixture, Value};

static ERROR_DROPPED: AtomicBool = AtomicBool::new(false);
static OVERFLOWED_ONCE: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct ArtificialError;

impl fmt::Display for ArtificialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("artificial failure")
    }
}

impl Error for ArtificialError {}

#[derive(Debug)]
struct InteriorNulError;

impl fmt::Display for InteriorNulError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("before\0after")
    }
}

impl Error for InteriorNulError {}

impl Drop for ArtificialError {
    fn drop(&mut self) {
        ERROR_DROPPED.store(true, Ordering::SeqCst);
    }
}

fn returns_error(
    _: &mut rsgdll_lua::StackFrame<'_, '_>,
    returns: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    returns.push(41.0_f64)?;
    Err(Box::new(ArtificialError))
}

fn panics(
    _: &mut rsgdll_lua::StackFrame<'_, '_>,
    _: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    panic!("artificial panic")
}

fn succeeds(
    _: &mut rsgdll_lua::StackFrame<'_, '_>,
    returns: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    returns.push(42.0_f64)?;
    Ok(())
}

fn returns_interior_nul_error(
    _: &mut rsgdll_lua::StackFrame<'_, '_>,
    _: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    Err(Box::new(InteriorNulError))
}

fn changes_stack(
    frame: &mut rsgdll_lua::StackFrame<'_, '_>,
    _: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    frame.push(42.0_f64)?;
    Ok(())
}

fn overflows_stack_once(
    frame: &mut rsgdll_lua::StackFrame<'_, '_>,
    returns: &mut ReturnWriter<'_>,
) -> Result<(), BoxError> {
    if !OVERFLOWED_ONCE.swap(true, Ordering::SeqCst) {
        for _ in 0..129 {
            frame.push(())?;
        }
        return Err(Box::new(ArtificialError));
    }
    returns.push(42.0_f64)?;
    Ok(())
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
    assert_eq!(
        rsgdll_bridge::__private::last_dispatch_status(),
        STATUS_RUST_ERROR
    );
    assert!(ERROR_DROPPED.load(Ordering::SeqCst));
    assert_eq!(fixture.stack(), &[Value::Number(7.0)]);
    assert_report(fixture.error(), "module.test_error: artificial failure");
}

#[test]
fn interior_nul_error_reaches_cpp_as_printable_text() {
    install_dispatcher();
    let id = register_callback("module.interior_nul", returns_interior_nul_error)
        .expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { trampoline()(fixture.state()) }, 0);

    assert_report(fixture.error(), "module.interior_nul: before\\0after");
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
    assert_eq!(
        rsgdll_bridge::__private::last_dispatch_status(),
        STATUS_RUST_PANIC
    );
    assert_eq!(fixture.stack(), &[Value::Number(7.0)]);
    assert_report(
        fixture.error(),
        "panic in module.test_panic: artificial panic",
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
fn callback_stack_mutation_is_reported_and_restored() {
    install_dispatcher();
    let id = register_callback("module.bad_stack", changes_stack).expect("callback registration");
    let mut fixture = Fixture::new(id.get(), vec![]);

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let returned = unsafe { trampoline()(fixture.state()) };

    assert_eq!(returned, 0);
    assert_eq!(
        rsgdll_bridge::__private::last_dispatch_status(),
        STATUS_INTERNAL_ERROR
    );
    assert_eq!(fixture.stack(), &[]);
    assert_report(
        fixture.error(),
        "module.bad_stack: callback declared 0 return values but left 1 on the stack",
    );
}

#[test]
fn stack_overflow_cleanup_allows_the_next_callback() {
    // Given: one callback crosses the protected context's soft stack limit.
    OVERFLOWED_ONCE.store(false, Ordering::SeqCst);
    install_dispatcher();
    let id =
        register_callback("module.stack_overflow", overflows_stack_once).expect("registration");
    let mut fixture = Fixture::new(id.get(), vec![]);

    // When: the overflowing call returns through the error path.
    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    let first = unsafe { trampoline()(fixture.state()) };
    // SAFETY: same fixture remains live after the recoverable failure.
    let second = unsafe { trampoline()(fixture.state()) };

    // Then: cleanup restores the stack and clears re-entry state.
    assert_eq!(first, 0);
    assert_eq!(second, 1);
    assert_eq!(fixture.stack(), &[Value::Number(42.0)]);
}

fn assert_report(actual: Option<&str>, expected: &str) {
    #[cfg(not(feature = "backtrace"))]
    assert_eq!(actual, Some(expected));

    #[cfg(feature = "backtrace")]
    {
        let actual = actual.expect("report is present");
        assert!(actual.starts_with(expected));
        assert!(actual.contains("\n\nRust backtrace:\n"));
    }
}
