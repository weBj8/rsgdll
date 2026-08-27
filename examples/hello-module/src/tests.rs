use super::gmod13_open;
use super::test_support::{Fixture, Value};
use super::{InitializationMode, set_initialization_mode};

#[test]
fn exported_module_functions_are_callable_through_lua_surface() {
    let mut fixture = Fixture::new();
    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 0);

    let (count, values, error) = fixture.call("hello", vec![Value::String(b"Ada".to_vec())]);
    assert_eq!(count, 1);
    assert_eq!(values, [Value::String(b"Hello Ada".to_vec())]);
    assert_eq!(error, None);

    let (count, values, error) = fixture.call("status", vec![]);
    assert_eq!(count, 2);
    assert_eq!(
        values,
        [Value::String(b"ready".to_vec()), Value::Bool(true)]
    );
    assert_eq!(error, None);

    let (count, values, error) = fixture.call("initialize", vec![]);
    assert_eq!(count, 0);
    assert!(values.is_empty());
    assert_eq!(error, None);

    let (count, values, error) = fixture.call("", vec![]);
    assert_eq!(count, 1);
    assert_eq!(values, [Value::String(Vec::new())]);
    assert_eq!(error, None);
}

#[test]
fn result_error_becomes_lua_error_after_rust_returns() {
    let mut fixture = Fixture::new();
    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 0);

    let (count, values, error) = fixture.call("get_user", vec![Value::Number(0.0)]);
    assert_eq!(count, 0);
    assert!(values.is_empty());
    let error = error.expect("Lua error");
    let expected = "rsgdll_example::get_user: user id must not be zero";
    assert!(
        error == expected
            || error
                .strip_prefix(expected)
                .is_some_and(|suffix| suffix.starts_with("\n\nRust backtrace:\n"))
    );
}

#[cfg(panic = "unwind")]
#[test]
fn initializer_panic_becomes_lua_error_after_rust_returns() {
    set_initialization_mode(InitializationMode::Panic);
    let mut fixture = Fixture::new();

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 0);

    assert!(
        fixture
            .error()
            .is_some_and(|error| error.contains("intentional initializer panic"))
    );
    assert!(!fixture.has_module_global());
    set_initialization_mode(InitializationMode::Normal);
}

#[test]
fn registration_failure_becomes_lua_error_after_rust_returns() {
    set_initialization_mode(InitializationMode::RegistrationFailure);
    let mut fixture = Fixture::new();

    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 0);

    assert!(
        fixture
            .error()
            .is_some_and(|error| error.contains("registration capacity"))
    );
    assert!(!fixture.has_module_global());
    set_initialization_mode(InitializationMode::Normal);
}
