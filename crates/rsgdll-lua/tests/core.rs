mod support;

use rsgdll_abi::{LuaCFunction, LuaType};
use rsgdll_lua::{FromLua, Lua, LuaError};
use support::{Fixture, Value};

unsafe extern "C" fn callback(_: *mut rsgdll_abi::RawLuaState) -> i32 {
    0
}

#[test]
fn conversion_reports_expected_and_actual_types() {
    // Given: an entity at stack index one.
    let mut fixture = Fixture::new(vec![Value::Entity], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");

    // When: Rust requests a String without calling a throwing Check* method.
    let error = String::from_lua(&lua, 1).expect_err("entity is not a string");

    // Then: the ordinary Rust error retains both types.
    assert_eq!(
        error,
        LuaError::TypeMismatch {
            expected: LuaType::STRING,
            actual: LuaType::ENTITY,
        }
    );
}

#[test]
fn primitives_round_trip_and_frame_restores_stack() {
    // Given: one pre-existing caller value.
    let mut fixture = Fixture::new(vec![Value::Entity], vec![]);
    {
        // SAFETY: fixture owns a live state and matching fake vtable for this test.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");

        // When: primitives are pushed and read inside a frame.
        {
            let mut stack = lua.stack();
            let mut frame = stack.frame();
            // SAFETY: fake operations cannot raise Lua errors or longjmp.
            unsafe {
                frame.push(true).expect("bool push");
                frame.push(42.5_f64).expect("number push");
                frame.push("hello").expect("string push");
                frame.push(()).expect("nil push");
            }
            assert!(frame.get::<bool>(-4).expect("bool"));
            assert_eq!(frame.get::<f64>(-3).expect("number"), 42.5);
            assert_eq!(frame.get::<String>(-2).expect("string"), "hello");
            assert_eq!(frame.get::<()>(-1).expect("nil"), ());
        }

        // Then: dropping the frame removes only values pushed inside it.
        assert_eq!(lua.stack().top(), 1);
    }
    assert_eq!(fixture.top(), 1);
}

#[test]
fn frame_rejects_pop_below_its_baseline() {
    // Given: a frame above one caller-owned value.
    let mut fixture = Fixture::new(vec![Value::Entity], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: safe code tries to pop the caller-owned value.
    let error = frame.pop(1).expect_err("frame must preserve caller values");

    // Then: Rust rejects the operation and leaves the stack unchanged.
    assert_eq!(
        error,
        LuaError::StackUnderflow {
            baseline: 1,
            requested_top: 0,
        }
    );
    assert_eq!(frame.top(), 1);
}

#[test]
fn raw_table_registration_and_closure_upvalues_are_available() {
    // Given: callback context with one numeric closure upvalue.
    let mut fixture = Fixture::new(vec![], vec![Value::Number(7.0)]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: registration creates a table and stores a C closure under a key.
    // SAFETY: fake operations cannot allocate, raise Lua errors, or longjmp.
    unsafe {
        frame.create_table();
        frame.push("handler").expect("key push");
        frame
            .push_c_function(callback as LuaCFunction)
            .expect("closure push");
        frame.raw_set(-3).expect("table assignment");
        frame.push("handler").expect("key push");
        frame.raw_get(-2).expect("table lookup");
    }

    // Then: closure type and first upvalue are inspectable without Check*.
    assert_eq!(frame.value_type(-1), LuaType::FUNCTION);
    assert_eq!(
        frame
            .get::<f64>(Lua::upvalue_index(1).expect("first upvalue"))
            .expect("numeric upvalue"),
        7.0
    );
}
