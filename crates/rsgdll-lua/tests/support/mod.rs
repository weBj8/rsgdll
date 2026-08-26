use std::collections::HashMap;
use std::ffi::{c_char, c_double, c_int, c_uint};
use std::ptr::NonNull;

use rsgdll_abi::{LuaCFunction, LuaType, RawLuaBase, RawLuaState, SpecialIndex};

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Table(HashMap<Vec<u8>, Value>),
    Function,
    Entity,
}

#[repr(C)]
struct TestState {
    prefix: [u8; 120],
    lua_base: *mut RawLuaBase,
    stack: Vec<Value>,
    upvalues: Vec<Value>,
}

#[repr(C)]
struct TestLuaBase {
    vtable: *const TestVTable,
    state: *mut RawLuaState,
}

type Slot = unsafe extern "C" fn();

#[repr(C)]
struct TestVTable {
    top: unsafe extern "C" fn(*mut RawLuaBase) -> c_int,
    push: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    pop: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_table: Slot,
    get_field: Slot,
    set_field: Slot,
    create_table: unsafe extern "C" fn(*mut RawLuaBase),
    set_table: Slot,
    set_meta_table: Slot,
    get_meta_table: Slot,
    call: Slot,
    pcall: Slot,
    equal: Slot,
    raw_equal: Slot,
    insert: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    remove: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    next: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_int,
    new_userdata: Slot,
    throw_error: Slot,
    check_type: Slot,
    arg_error: Slot,
    raw_get: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    raw_set: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_string: unsafe extern "C" fn(*mut RawLuaBase, c_int, *mut c_uint) -> *const c_char,
    get_number: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_double,
    get_bool: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> bool,
    get_c_function: Slot,
    get_userdata: Slot,
    push_nil: unsafe extern "C" fn(*mut RawLuaBase),
    push_string: unsafe extern "C" fn(*mut RawLuaBase, *const c_char, c_uint),
    push_number: unsafe extern "C" fn(*mut RawLuaBase, c_double),
    push_bool: unsafe extern "C" fn(*mut RawLuaBase, bool),
    push_c_function: Slot,
    push_c_closure: unsafe extern "C" fn(*mut RawLuaBase, LuaCFunction, c_int),
    push_userdata: Slot,
    reference_create: Slot,
    reference_free: Slot,
    reference_push: Slot,
    push_special: unsafe extern "C" fn(*mut RawLuaBase, SpecialIndex),
    is_type: Slot,
    get_type: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_int,
    get_type_name: Slot,
    create_meta_table_type: Slot,
    check_string: Slot,
    check_number: Slot,
    obj_len: Slot,
    get_angle: Slot,
    get_vector: Slot,
    push_angle: Slot,
    push_vector: Slot,
    set_state: unsafe extern "C" fn(*mut RawLuaBase, *mut RawLuaState),
    create_meta_table: Slot,
    push_meta_table: Slot,
    push_user_type: Slot,
    set_user_type: Slot,
}

pub struct Fixture {
    state: NonNull<TestState>,
    lua_base: NonNull<TestLuaBase>,
    vtable: NonNull<TestVTable>,
}

impl Fixture {
    pub fn new(stack: Vec<Value>, upvalues: Vec<Value>) -> Self {
        let vtable = NonNull::from(Box::leak(Box::new(test_vtable())));
        let lua_base = NonNull::from(Box::leak(Box::new(TestLuaBase {
            vtable: vtable.as_ptr(),
            state: std::ptr::null_mut(),
        })));
        let state = NonNull::from(Box::leak(Box::new(TestState {
            prefix: [0; 120],
            lua_base: lua_base.as_ptr().cast(),
            stack,
            upvalues,
        })));
        // SAFETY: raw-owned allocations remain live until `Fixture::drop`.
        unsafe { (*lua_base.as_ptr()).state = state.as_ptr().cast() };
        Self {
            state,
            lua_base,
            vtable,
        }
    }

    pub fn state(&mut self) -> *mut RawLuaState {
        self.state.as_ptr().cast()
    }

    pub fn top(&self) -> usize {
        // SAFETY: allocation remains live and callers inspect only when no
        // `Lua` access is active.
        unsafe { (*self.state.as_ptr()).stack.len() }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: `state` came from one `Box::leak` and is reclaimed once.
        unsafe { drop(Box::from_raw(self.state.as_ptr())) };
        // SAFETY: `lua_base` came from one `Box::leak` and is reclaimed once.
        unsafe { drop(Box::from_raw(self.lua_base.as_ptr())) };
        // SAFETY: `vtable` came from one `Box::leak` and is reclaimed once.
        unsafe { drop(Box::from_raw(self.vtable.as_ptr())) };
    }
}

unsafe extern "C" fn unused() {}

unsafe fn test_state<'a>(lua_base: *mut RawLuaBase) -> &'a mut TestState {
    // SAFETY: test fixtures pass a live `TestLuaBase` with matching C layout.
    let state = unsafe { (*(lua_base.cast::<TestLuaBase>())).state };
    // SAFETY: fixture state remains allocated and exclusively accessed through
    // the emulated Lua calls while a `Lua` handle exists.
    unsafe { &mut *state.cast::<TestState>() }
}

fn stack_offset(state: &TestState, index: c_int) -> Option<usize> {
    if index > 0 {
        usize::try_from(index - 1)
            .ok()
            .filter(|offset| *offset < state.stack.len())
    } else if index < 0 && index > -10_000 {
        usize::try_from(state.stack.len() as isize + index as isize)
            .ok()
            .filter(|offset| *offset < state.stack.len())
    } else {
        None
    }
}

fn value(state: &TestState, index: c_int) -> Option<&Value> {
    if index <= -10_003 {
        usize::try_from(-10_003_i64 - i64::from(index))
            .ok()
            .and_then(|offset| state.upvalues.get(offset))
    } else {
        stack_offset(state, index).and_then(|offset| state.stack.get(offset))
    }
}

unsafe extern "C" fn top(lua_base: *mut RawLuaBase) -> c_int {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }.stack.len() as c_int
}

unsafe extern "C" fn push(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    if let Some(value) = value(state, index).cloned() {
        state.stack.push(value);
    }
}

unsafe extern "C" fn pop(lua_base: *mut RawLuaBase, count: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    state.stack.truncate(state.stack.len() - count as usize);
}

unsafe extern "C" fn create_table(lua_base: *mut RawLuaBase) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .stack
        .push(Value::Table(HashMap::new()));
}

unsafe extern "C" fn raw_get(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let table_offset = stack_offset(state, index);
    let key = state.stack.pop();
    let result = match (table_offset, key) {
        (Some(offset), Some(Value::String(key))) => match state.stack.get(offset) {
            Some(Value::Table(table)) => table.get(&key).cloned().unwrap_or(Value::Nil),
            _ => Value::Nil,
        },
        _ => Value::Nil,
    };
    state.stack.push(result);
}

unsafe extern "C" fn raw_set(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let table_offset = stack_offset(state, index);
    let assigned = state.stack.pop();
    let key = state.stack.pop();
    if let (Some(offset), Some(Value::String(key)), Some(assigned)) = (table_offset, key, assigned)
        && let Some(Value::Table(table)) = state.stack.get_mut(offset)
    {
        table.insert(key, assigned);
    }
}

unsafe extern "C" fn get_string(
    lua_base: *mut RawLuaBase,
    index: c_int,
    output_length: *mut c_uint,
) -> *const c_char {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let Some(Value::String(bytes)) = value(state, index) else {
        return std::ptr::null();
    };
    if !output_length.is_null() {
        // SAFETY: caller supplied writable storage for one `c_uint`.
        unsafe { output_length.write(bytes.len() as c_uint) };
    }
    bytes.as_ptr().cast()
}

unsafe extern "C" fn get_number(lua_base: *mut RawLuaBase, index: c_int) -> c_double {
    // SAFETY: forwarded from a live fixture vtable.
    match value(unsafe { test_state(lua_base) }, index) {
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    }
}

unsafe extern "C" fn get_bool(lua_base: *mut RawLuaBase, index: c_int) -> bool {
    // SAFETY: forwarded from a live fixture vtable.
    matches!(
        value(unsafe { test_state(lua_base) }, index),
        Some(Value::Bool(true))
    )
}

unsafe extern "C" fn push_nil(lua_base: *mut RawLuaBase) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }.stack.push(Value::Nil);
}

unsafe extern "C" fn push_string(lua_base: *mut RawLuaBase, bytes: *const c_char, length: c_uint) {
    // SAFETY: wrapper supplies a readable buffer of exactly `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(bytes.cast(), length as usize) };
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .stack
        .push(Value::String(bytes.to_vec()));
}

unsafe extern "C" fn push_number(lua_base: *mut RawLuaBase, number: c_double) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .stack
        .push(Value::Number(number));
}

unsafe extern "C" fn push_bool(lua_base: *mut RawLuaBase, value: bool) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .stack
        .push(Value::Bool(value));
}

unsafe extern "C" fn push_c_closure(
    lua_base: *mut RawLuaBase,
    _: LuaCFunction,
    upvalue_count: c_int,
) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    state
        .stack
        .truncate(state.stack.len() - upvalue_count as usize);
    state.stack.push(Value::Function);
}

unsafe extern "C" fn push_special(_: *mut RawLuaBase, _: SpecialIndex) {}

unsafe extern "C" fn get_type(lua_base: *mut RawLuaBase, index: c_int) -> c_int {
    // SAFETY: forwarded from a live fixture vtable.
    match value(unsafe { test_state(lua_base) }, index) {
        None => LuaType::NONE.0,
        Some(Value::Nil) => LuaType::NIL.0,
        Some(Value::Bool(_)) => LuaType::BOOL.0,
        Some(Value::Number(_)) => LuaType::NUMBER.0,
        Some(Value::String(_)) => LuaType::STRING.0,
        Some(Value::Table(_)) => LuaType::TABLE.0,
        Some(Value::Function) => LuaType::FUNCTION.0,
        Some(Value::Entity) => LuaType::ENTITY.0,
    }
}

unsafe extern "C" fn set_state(lua_base: *mut RawLuaBase, state: *mut RawLuaState) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { (*(lua_base.cast::<TestLuaBase>())).state = state };
}

unsafe extern "C" fn int(_: *mut RawLuaBase, _: c_int) {}

unsafe extern "C" fn int_result(_: *mut RawLuaBase, _: c_int) -> c_int {
    0
}

fn test_vtable() -> TestVTable {
    TestVTable {
        top,
        push,
        pop,
        get_table: unused,
        get_field: unused,
        set_field: unused,
        create_table,
        set_table: unused,
        set_meta_table: unused,
        get_meta_table: unused,
        call: unused,
        pcall: unused,
        equal: unused,
        raw_equal: unused,
        insert: int,
        remove: int,
        next: int_result,
        new_userdata: unused,
        throw_error: unused,
        check_type: unused,
        arg_error: unused,
        raw_get,
        raw_set,
        get_string,
        get_number,
        get_bool,
        get_c_function: unused,
        get_userdata: unused,
        push_nil,
        push_string,
        push_number,
        push_bool,
        push_c_function: unused,
        push_c_closure,
        push_userdata: unused,
        reference_create: unused,
        reference_free: unused,
        reference_push: unused,
        push_special,
        is_type: unused,
        get_type,
        get_type_name: unused,
        create_meta_table_type: unused,
        check_string: unused,
        check_number: unused,
        obj_len: unused,
        get_angle: unused,
        get_vector: unused,
        push_angle: unused,
        push_vector: unused,
        set_state,
        create_meta_table: unused,
        push_meta_table: unused,
        push_user_type: unused,
        set_user_type: unused,
    }
}
