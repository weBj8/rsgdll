mod support;

use std::cell::Cell;
use std::rc::Rc;

use rsgdll_abi::{LuaCFunction, LuaType};
use rsgdll_lua::{FromLua, Lua, LuaBytes, LuaError};
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
fn u64_conversion_accepts_only_exact_nonnegative_lua_integers() {
    let mut valid = Fixture::new(vec![Value::Number(42.0)], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let lua = unsafe { Lua::from_raw(valid.state()) }.expect("valid fixture");
    assert_eq!(u64::from_lua(&lua, 1).expect("exact integer"), 42);

    let mut fractional = Fixture::new(vec![Value::Number(1.5)], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let lua = unsafe { Lua::from_raw(fractional.state()) }.expect("valid fixture");
    assert_eq!(
        u64::from_lua(&lua, 1).expect_err("fractional number"),
        LuaError::IntegerOutOfRange
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
fn binary_strings_round_trip_without_utf8_or_nul_loss() {
    // Given: arbitrary Lua string bytes, including invalid UTF-8 and NUL.
    let bytes = vec![0, 0xff, b'A', 0];
    let mut fixture = Fixture::new(vec![Value::String(bytes.clone())], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: bytes are read and pushed through the binary string API.
    let value = frame.get::<LuaBytes>(1).expect("binary string");
    // SAFETY: fake push copies bytes and cannot raise or longjmp.
    unsafe { frame.push(value.clone()).expect("binary push") };

    // Then: every byte survives exactly.
    assert_eq!(value.as_bytes(), bytes);
    assert_eq!(
        frame.get::<LuaBytes>(-1).expect("pushed binary").as_bytes(),
        bytes
    );
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

#[test]
fn owned_table_handles_support_typed_get_set_and_push() {
    // Given: an empty callback stack.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: Rust creates a registry-backed table and mutates it.
    // SAFETY: fake table/reference operations cannot allocate, raise, or longjmp.
    let table = unsafe { frame.new_table().expect("new table") };
    // SAFETY: fake pushes and raw table operations cannot raise or longjmp.
    unsafe {
        table
            .set(&mut frame, "answer", 42.0_f64)
            .expect("table set");
    }

    // Then: typed lookup works and the table can be returned to Lua.
    // SAFETY: fake reference push and raw lookup cannot raise or longjmp.
    let answer = unsafe {
        table
            .get::<_, f64>(&mut frame, "answer")
            .expect("table get")
    };
    assert_eq!(answer, 42.0);
    // SAFETY: fake reference push cannot raise or longjmp.
    unsafe { table.push(&mut frame).expect("table push") };
    assert_eq!(frame.value_type(-1), LuaType::TABLE);
}

#[test]
fn owned_function_handles_preserve_callable_values() {
    // Given: a Lua function at the first argument.
    let mut fixture = Fixture::new(vec![Value::Function], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: Rust captures it in a state-owned function handle.
    // SAFETY: fake reference operations cannot allocate, raise, or longjmp.
    let function = unsafe { frame.function(1).expect("function handle") };
    frame.pop(0).expect("no-op pop");
    // SAFETY: fake reference push cannot raise or longjmp.
    unsafe { function.push(&mut frame).expect("function push") };

    // Then: it remains a function after registry storage.
    assert_eq!(frame.value_type(-1), LuaType::FUNCTION);
}

#[test]
fn protected_call_returns_lua_errors_as_rust_results() {
    // Given: a Lua function that raises an error.
    let mut fixture = Fixture::new(
        vec![Value::FunctionError(b"lua callback failed".to_vec())],
        vec![],
    );
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    // SAFETY: fake registry operations cannot allocate, raise, or longjmp.
    let function = unsafe { frame.function(1).expect("function handle") };

    // When: Rust invokes it through protected calling behavior.
    // SAFETY: fake pushes cannot raise; fake PCall catches the emulated error.
    let error = unsafe {
        function
            .call::<_, ()>(&mut frame, ())
            .expect_err("Lua error must become Result::Err")
    };

    // Then: error status and binary-safe message return through Rust.
    assert_eq!(
        error,
        LuaError::Call {
            status: 1,
            message: LuaBytes::from(b"lua callback failed".to_vec()),
        }
    );
}

#[test]
fn protected_call_decodes_multiple_results_in_order() {
    // Given: a Lua function returning three unlike values.
    let mut fixture = Fixture::new(
        vec![Value::FunctionReturns(vec![
            Value::String(b"one".to_vec()),
            Value::Number(2.0),
            Value::Bool(true),
        ])],
        vec![],
    );
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    // SAFETY: fake registry operations cannot allocate, raise, or longjmp.
    let function = unsafe { frame.function(1).expect("function handle") };

    // When: Rust requests the matching result tuple.
    // SAFETY: fake pushes cannot raise; fake PCall is protected.
    let (text, number, flag) = unsafe {
        function
            .call::<_, (LuaBytes, f64, bool)>(&mut frame, ())
            .expect("multiple Lua returns")
    };

    // Then: count, ordering, and types remain exact.
    assert_eq!(text.as_bytes(), b"one");
    assert_eq!(number, 2.0);
    assert!(flag);
}

#[test]
fn registry_reference_releases_its_slot_exactly_on_drop() {
    // Given: one Lua value and no registry entries.
    let mut fixture = Fixture::new(vec![Value::String(b"kept".to_vec())], vec![]);
    {
        // SAFETY: fixture owns a live state and matching fake vtable.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
        let mut stack = lua.stack();
        let mut frame = stack.frame();

        // When: a callback-scoped reference is created and then dropped.
        // SAFETY: fake registry operations cannot allocate, raise, or longjmp.
        let reference = unsafe { frame.create_reference(1).expect("registry reference") };
        drop(reference);
    }

    // Then: its registry slot is no longer owned.
    assert_eq!(fixture.reference_count(), 0);
}

#[test]
fn typed_userdata_storage_supports_checked_shared_access() {
    // Given: a registered Rust userdata type.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    // SAFETY: fake metatable creation cannot allocate, raise, or longjmp.
    let kind = unsafe {
        frame
            .userdata_type::<Cell<u64>>("rsgdll.test.Counter")
            .expect("userdata type")
    };

    // When: Rust stores one value in full userdata.
    // SAFETY: fake userdata/metatable operations cannot raise or longjmp.
    unsafe {
        kind.push(&mut frame, Cell::new(7)).expect("userdata push");
    }

    // Then: checked access sees the original Rust value.
    let value = kind.borrow(&frame, -1).expect("userdata borrow");
    assert_eq!(value.get(), 7);
}

#[test]
fn userdata_type_exposes_self_indexing_metatable() {
    // Given: a newly registered userdata type.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    // SAFETY: fake metatable creation cannot allocate, raise, or longjmp.
    let kind = unsafe {
        frame
            .userdata_type::<Cell<u64>>("rsgdll.test.Metatable")
            .expect("userdata type")
    };

    // When: Rust pushes its metatable and reads __index.
    // SAFETY: fake metatable/table operations cannot raise or longjmp.
    unsafe {
        kind.push_metatable(&mut frame).expect("metatable");
        frame.push("__index").expect("key");
        frame.raw_get(-2).expect("index lookup");
    }

    // Then: method lookup delegates to the metatable itself.
    assert_eq!(frame.value_type(-1), LuaType::TABLE);
}

#[test]
fn userdata_finalization_drops_rust_value_exactly_once() {
    struct DropProbe(Rc<Cell<usize>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    // Given: one Lua-owned Rust value.
    let drops = Rc::new(Cell::new(0));
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    // SAFETY: fake metatable creation cannot allocate, raise, or longjmp.
    let kind = unsafe {
        frame
            .userdata_type::<DropProbe>("rsgdll.test.DropProbe")
            .expect("userdata type")
    };
    // SAFETY: fake userdata/metatable operations cannot raise or longjmp.
    unsafe {
        kind.push(&mut frame, DropProbe(Rc::clone(&drops)))
            .expect("userdata push");
    }

    // When: the userdata finalizer runs twice.
    kind.finalize(&mut frame, -1).expect("first finalize");
    let second = kind
        .finalize(&mut frame, -1)
        .expect_err("second finalize rejected");

    // Then: Rust ownership is released exactly once.
    assert_eq!(drops.get(), 1);
    assert_eq!(second, LuaError::FinalizedUserData);
}
