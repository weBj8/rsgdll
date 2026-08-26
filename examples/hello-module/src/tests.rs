use super::gmod13_open;
use super::test_support::{Fixture, Value};

#[test]
fn exported_module_functions_are_callable_through_lua_surface() {
    let mut fixture = Fixture::new();
    // SAFETY: fixture provides a live pinned-layout state and fake vtable.
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 1);

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
    assert_eq!(unsafe { gmod13_open(fixture.state()) }, 1);

    let (count, values, error) = fixture.call("get_user", vec![Value::Number(0.0)]);
    assert_eq!(count, 0);
    assert!(values.is_empty());
    assert_eq!(
        error.as_deref(),
        Some("rsgdll_example::get_user: user id must not be zero")
    );
}
