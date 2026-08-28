mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rsgdll_abi::{LuaCFunction, LuaType};
use rsgdll_lua::{FromLua, IntoLua, IntoLuaMulti, Lua, LuaBytes, LuaError, LuaResult, StackFrame};
use support::{Fixture, LuaTestExt, Value};

unsafe extern "C" fn callback(_: *mut rsgdll_abi::RawLuaState) -> i32 {
    0
}

struct MissingArgument;
struct PushThenFail;

impl IntoLua for PushThenFail {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        true.into_lua(lua)?;
        Err(LuaError::CountOverflow)
    }
}

impl IntoLuaMulti for MissingArgument {
    fn count(&self) -> usize {
        1
    }

    fn push(self, _: &mut StackFrame<'_, '_>) -> LuaResult<()> {
        Ok(())
    }
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
fn integer_conversions_validate_lua_numbers_and_rust_ranges() {
    let mut valid = Fixture::new(
        vec![
            Value::Number(255.0),
            Value::Number(65_535.0),
            Value::Number(4_294_967_295.0),
            Value::Number(9_007_199_254_740_992.0),
            Value::Number(-128.0),
            Value::Number(-32_768.0),
            Value::Number(-2_147_483_648.0),
            Value::Number(-9_007_199_254_740_992.0),
        ],
        vec![],
    );
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let lua = unsafe { Lua::from_raw(valid.state()) }.expect("valid fixture");
    assert_eq!(u8::from_lua(&lua, 1).expect("u8"), u8::MAX);
    assert_eq!(u16::from_lua(&lua, 2).expect("u16"), u16::MAX);
    assert_eq!(u32::from_lua(&lua, 3).expect("u32"), u32::MAX);
    assert_eq!(
        u64::from_lua(&lua, 4).expect("largest exact u64"),
        1_u64 << 53
    );
    assert_eq!(i8::from_lua(&lua, 5).expect("i8"), i8::MIN);
    assert_eq!(i16::from_lua(&lua, 6).expect("i16"), i16::MIN);
    assert_eq!(i32::from_lua(&lua, 7).expect("i32"), i32::MIN);
    assert_eq!(
        i64::from_lua(&lua, 8).expect("smallest exact i64"),
        -(1_i64 << 53)
    );

    let mut invalid = Fixture::new(
        vec![
            Value::Number(256.0),
            Value::Number(-129.0),
            Value::Number(1.5),
            Value::Number(f64::INFINITY),
        ],
        vec![],
    );
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let lua = unsafe { Lua::from_raw(invalid.state()) }.expect("valid fixture");
    assert_eq!(u8::from_lua(&lua, 1), Err(LuaError::IntegerOutOfRange));
    assert_eq!(i8::from_lua(&lua, 2), Err(LuaError::IntegerOutOfRange));
    assert_eq!(u32::from_lua(&lua, 3), Err(LuaError::IntegerOutOfRange));
    assert_eq!(i64::from_lua(&lua, 4), Err(LuaError::IntegerOutOfRange));
}

#[test]
fn rust_integers_push_as_exact_lua_numbers() {
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable for this test.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    frame.push(u8::MAX).expect("u8");
    frame.push(u16::MAX).expect("u16");
    frame.push(u32::MAX).expect("u32");
    frame.push(1_u64 << 53).expect("largest exact u64");
    frame.push(i8::MIN).expect("i8");
    frame.push(i16::MIN).expect("i16");
    frame.push(i32::MIN).expect("i32");
    frame.push(-(1_i64 << 53)).expect("smallest exact i64");

    assert_eq!(frame.get::<u8>(-8).expect("u8"), u8::MAX);
    assert_eq!(frame.get::<u16>(-7).expect("u16"), u16::MAX);
    assert_eq!(frame.get::<u32>(-6).expect("u32"), u32::MAX);
    assert_eq!(frame.get::<u64>(-5).expect("u64"), 1_u64 << 53);
    assert_eq!(frame.get::<i8>(-4).expect("i8"), i8::MIN);
    assert_eq!(frame.get::<i16>(-3).expect("i16"), i16::MIN);
    assert_eq!(frame.get::<i32>(-2).expect("i32"), i32::MIN);
    assert_eq!(frame.get::<i64>(-1).expect("i64"), -(1_i64 << 53));

    assert_eq!(
        frame.push((1_u64 << 53) + 1),
        Err(LuaError::IntegerOutOfRange)
    );
    assert_eq!(
        frame.push(-(1_i64 << 53) - 1),
        Err(LuaError::IntegerOutOfRange)
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
            frame.push(true).expect("bool push");
            frame.push(42.5_f64).expect("number push");
            frame.push("hello").expect("string push");
            frame.push(()).expect("nil push");
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
    frame.push(value.clone()).expect("binary push");

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
    frame.create_table().expect("table");
    frame.push("handler").expect("key push");
    // SAFETY: callback has the exact Lua C ABI and cannot unwind.
    unsafe {
        rsgdll_lua::__private::push_c_closure(&mut frame, callback as LuaCFunction, 0)
            .expect("closure push");
    }
    frame.raw_set(-3).expect("table assignment");
    frame.push("handler").expect("key push");
    frame.raw_get(-2).expect("table lookup");

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
    let table = frame.new_table().expect("new table");
    table
        .set(&mut frame, "answer", 42.0_f64)
        .expect("table set");

    // Then: typed lookup works and the table can be returned to Lua.
    let answer = table
        .get::<_, f64>(&mut frame, "answer")
        .expect("table get");
    assert_eq!(answer, 42.0);
    table.push(&mut frame).expect("table push");
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
    let function = frame.function(1).expect("function handle");
    frame.pop(0).expect("no-op pop");
    function.push(&mut frame).expect("function push");

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
    let function = frame.function(1).expect("function handle");

    // When: Rust invokes it through protected calling behavior.
    let error = function
        .call::<_, ()>(&mut frame, ())
        .expect_err("Lua error must become Result::Err");

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
    let function = frame.function(1).expect("function handle");

    // When: Rust requests the matching result tuple.
    let (text, number, flag) = function
        .call::<_, (LuaBytes, f64, bool)>(&mut frame, ())
        .expect("multiple Lua returns");

    // Then: count, ordering, and types remain exact.
    assert_eq!(text.as_bytes(), b"one");
    assert_eq!(number, 2.0);
    assert!(flag);
}

#[test]
fn protected_call_rejects_argument_count_without_matching_stack_values() {
    // Given: a safe argument conversion claims one value but pushes none.
    let mut fixture = Fixture::new(vec![Value::Function], vec![]);

    {
        // SAFETY: fixture owns a live state and matching fake vtable.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
        let mut stack = lua.stack();
        let mut frame = stack.frame();
        let function = frame.function(1).expect("function handle");

        // When: Rust prepares the protected call.
        let result = function.call::<_, ()>(&mut frame, MissingArgument);

        // Then: validation fails before C++ consumes invalid stack indices.
        assert_eq!(
            result,
            Err(LuaError::ArgumentCountMismatch {
                expected: 1,
                actual: 0,
            })
        );
    }

    assert_eq!(fixture.top(), 1);
}

#[test]
fn protected_call_restores_stack_after_argument_conversion_failure() {
    // Given: one callable and an argument conversion that pushes before failing.
    let mut fixture = Fixture::new(vec![Value::Function], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    let function = frame.function(1).expect("function handle");
    let entry_top = frame.top();

    // When: argument conversion fails after mutating the Lua stack.
    let error = function
        .call::<_, ()>(&mut frame, (PushThenFail,))
        .expect_err("argument conversion");

    // Then: the error remains catchable without leaving temporary values.
    assert_eq!(error, LuaError::CountOverflow);
    assert_eq!(frame.top(), entry_top);
}

#[test]
fn table_set_restores_stack_after_value_conversion_failure() {
    // Given: one table and a value conversion that pushes before failing.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    let table = frame.new_table().expect("table");
    let entry_top = frame.top();

    // When: value conversion fails after mutating the Lua stack.
    let error = table
        .set(&mut frame, "key", PushThenFail)
        .expect_err("value conversion");

    // Then: the error remains catchable without leaving temporary values.
    assert_eq!(error, LuaError::CountOverflow);
    assert_eq!(frame.top(), entry_top);
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
        let reference = frame.create_reference(1).expect("registry reference");
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
    let kind = frame
        .userdata_type::<Cell<u64>>("rsgdll.test.Counter")
        .expect("userdata type");

    // When: Rust stores one value in full userdata.
    kind.push(&mut frame, Cell::new(7)).expect("userdata push");

    // Then: checked access sees the original Rust value.
    let value = kind.borrow(&frame, -1).expect("userdata borrow");
    assert_eq!(value.get(), 7);
}

#[test]
fn typed_userdata_rejects_unowned_foreign_storage() {
    // Given: a foreign userdata header that reuses this Rust type's Lua tag.
    let mut fixture = Fixture::new(vec![], vec![]);
    let lua_type = {
        // SAFETY: fixture owns a live state and matching fake vtable.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
        let mut stack = lua.stack();
        let mut frame = stack.frame();
        let kind = frame
            .userdata_type::<Cell<u64>>("rsgdll.test.Foreign")
            .expect("userdata type");
        rsgdll_lua::__private::userdata_type_id(&kind)
    };
    let foreign = Box::into_raw(Box::new(RefCell::new(Cell::new(7_u64))));
    fixture.push_foreign_userdata(lua_type, foreign.cast());

    // When: checked access receives storage not allocated by this module.
    let error = {
        // SAFETY: fixture still owns the same live state and matching fake vtable.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
        let mut stack = lua.stack();
        let mut frame = stack.frame();
        let kind = frame
            .userdata_type::<Cell<u64>>("rsgdll.test.Foreign")
            .expect("userdata type");
        match kind.borrow(&frame, 1) {
            Ok(value) => {
                drop(value);
                None
            }
            Err(error) => Some(error),
        }
    };
    // SAFETY: framework must reject foreign ownership; this allocation remains ours.
    unsafe { drop(Box::from_raw(foreign)) };

    // Then: no foreign pointer is interpreted as a Rust allocation.
    assert_eq!(error, Some(LuaError::UserDataTypeMismatch));
}

#[test]
fn typed_userdata_rejects_generic_userdata_before_header_access() {
    // Given: generic userdata with no framework-owned RawUserData header.
    let mut fixture = Fixture::new(vec![Value::GenericUserData(std::ptr::null_mut())], vec![]);

    {
        // SAFETY: fixture owns a live state and matching fake vtable.
        let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
        let mut stack = lua.stack();
        let mut frame = stack.frame();
        let kind = frame
            .userdata_type::<Cell<u64>>("rsgdll.test.GenericForeign")
            .expect("userdata type");

        // When: checked borrow and finalization receive generic userdata.
        let borrow_error = match kind.borrow(&frame, 1) {
            Ok(value) => {
                drop(value);
                panic!("generic userdata was accepted");
            }
            Err(error) => error,
        };
        let finalize_error = kind
            .finalize(&mut frame, 1)
            .expect_err("generic userdata finalization must be rejected");

        // Then: both safe APIs reject it as unowned storage.
        assert_eq!(borrow_error, LuaError::UserDataTypeMismatch);
        assert_eq!(finalize_error, LuaError::UserDataTypeMismatch);
    }

    assert_eq!(fixture.get_userdata_calls(), 0);
}

#[test]
fn userdata_type_exposes_self_indexing_metatable() {
    // Given: a newly registered userdata type.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    let kind = frame
        .userdata_type::<Cell<u64>>("rsgdll.test.Metatable")
        .expect("userdata type");

    // When: Rust pushes its metatable and reads __index.
    kind.push_metatable(&mut frame).expect("metatable");
    frame.push("__index").expect("key");
    frame.raw_get(-2).expect("index lookup");

    // Then: method lookup delegates to the metatable itself.
    assert_eq!(frame.value_type(-1), LuaType::TABLE);
}

#[test]
fn userdata_registration_installs_gc_before_creation() {
    // Given: a newly registered Rust userdata type with no separate GC setup.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    let kind = frame
        .userdata_type::<Cell<u64>>("rsgdll.test.AutomaticGc")
        .expect("userdata type");

    // When: the metatable is inspected before any userdata is created.
    kind.push_metatable(&mut frame).expect("metatable");
    frame.push("__gc").expect("key");
    frame.raw_get(-2).expect("GC lookup");

    // Then: registration has already installed a callable finalizer.
    assert_eq!(frame.value_type(-1), LuaType::FUNCTION);
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
    let kind = frame
        .userdata_type::<DropProbe>("rsgdll.test.DropProbe")
        .expect("userdata type");
    kind.push(&mut frame, DropProbe(Rc::clone(&drops)))
        .expect("userdata push");

    // When: the userdata finalizer runs twice.
    kind.finalize(&mut frame, -1).expect("first finalize");
    let second = kind
        .finalize(&mut frame, -1)
        .expect_err("second finalize rejected");

    // Then: Rust ownership is released exactly once.
    assert_eq!(drops.get(), 1);
    assert_eq!(second, LuaError::FinalizedUserData);
}
