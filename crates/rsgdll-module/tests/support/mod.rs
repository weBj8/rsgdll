use std::ffi::{c_char, c_double, c_int};
use std::ptr::NonNull;

use rsgdll_abi::{LuaType, RawLuaBase, RawLuaState};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Number(f64),
    Bool(bool),
    Executor,
    Registry,
    GuardKey,
}

#[repr(C)]
struct TestState {
    prefix: [u8; 120],
    lua_base: *mut RawLuaBase,
    stack: Vec<Value>,
    callback_id: f64,
    error: Option<String>,
    executor: Option<rsgdll_abi::LuaCFunction>,
    guard_active: bool,
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
    push: Slot,
    pop: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_table: Slot,
    get_field: Slot,
    set_field: Slot,
    create_table: Slot,
    set_table: Slot,
    set_meta_table: Slot,
    get_meta_table: Slot,
    call: Slot,
    pcall: unsafe extern "C" fn(*mut RawLuaBase, c_int, c_int, c_int) -> c_int,
    equal: Slot,
    raw_equal: Slot,
    insert: Slot,
    remove: Slot,
    next: Slot,
    new_userdata: Slot,
    throw_error: unsafe extern "C" fn(*mut RawLuaBase, *const c_char),
    check_type: Slot,
    arg_error: Slot,
    raw_get: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    raw_set: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_string: Slot,
    get_number: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_double,
    get_bool: Slot,
    get_c_function: Slot,
    get_userdata: Slot,
    push_nil: unsafe extern "C" fn(*mut RawLuaBase),
    push_string: unsafe extern "C" fn(*mut RawLuaBase, *const c_char, u32),
    push_number: unsafe extern "C" fn(*mut RawLuaBase, c_double),
    push_bool: unsafe extern "C" fn(*mut RawLuaBase, bool),
    push_c_function: Slot,
    push_c_closure: unsafe extern "C" fn(*mut RawLuaBase, rsgdll_abi::LuaCFunction, c_int),
    push_userdata: Slot,
    reference_create: unsafe extern "C" fn(*mut RawLuaBase) -> c_int,
    reference_free: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    reference_push: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    push_special: unsafe extern "C" fn(*mut RawLuaBase, c_int),
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
    set_state: Slot,
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
    pub fn new(callback_id: u32, stack: Vec<Value>) -> Self {
        let vtable = NonNull::from(Box::leak(Box::new(test_vtable())));
        let lua_base = NonNull::from(Box::leak(Box::new(TestLuaBase {
            vtable: vtable.as_ptr(),
            state: std::ptr::null_mut(),
        })));
        let state = NonNull::from(Box::leak(Box::new(TestState {
            prefix: [0; 120],
            lua_base: lua_base.as_ptr().cast(),
            stack,
            callback_id: f64::from(callback_id),
            error: None,
            executor: None,
            guard_active: false,
        })));
        // SAFETY: all allocations remain live until `Fixture::drop`.
        unsafe { (*lua_base.as_ptr()).state = state.as_ptr().cast() };
        // SAFETY: fixture setup vtable methods are deterministic Rust fakes
        // that never raise Lua errors or perform `longjmp`.
        unsafe { rsgdll_bridge::__private::enable_test_mode() };
        Self {
            state,
            lua_base,
            vtable,
        }
    }

    pub fn state(&mut self) -> *mut RawLuaState {
        self.state.as_ptr().cast()
    }

    pub fn stack(&self) -> &[Value] {
        // SAFETY: fixture exclusively owns live state.
        unsafe { &(*self.state.as_ptr()).stack }
    }

    pub fn error(&self) -> Option<&str> {
        // SAFETY: fixture exclusively owns live state.
        unsafe { (*self.state.as_ptr()).error.as_deref() }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: each pointer came from one `Box::leak` and is reclaimed once.
        unsafe {
            drop(Box::from_raw(self.state.as_ptr()));
            drop(Box::from_raw(self.lua_base.as_ptr()));
            drop(Box::from_raw(self.vtable.as_ptr()));
        }
    }
}

unsafe fn state<'a>(lua_base: *mut RawLuaBase) -> &'a mut TestState {
    // SAFETY: fake vtable receives only its matching live object.
    unsafe { &mut *((*lua_base.cast::<TestLuaBase>()).state.cast::<TestState>()) }
}

unsafe extern "C" fn unused() {}

unsafe extern "C" fn top(lua_base: *mut RawLuaBase) -> c_int {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.len() as c_int
}

unsafe extern "C" fn pop(lua_base: *mut RawLuaBase, count: c_int) {
    // SAFETY: dispatcher only pops values it observed above baseline.
    let state = unsafe { state(lua_base) };
    state.stack.truncate(state.stack.len() - count as usize);
}

unsafe extern "C" fn get_number(lua_base: *mut RawLuaBase, index: c_int) -> c_double {
    if index == -10_003 {
        // SAFETY: forwarded from matching fake vtable.
        unsafe { state(lua_base) }.callback_id
    } else {
        0.0
    }
}

unsafe extern "C" fn push_number(lua_base: *mut RawLuaBase, value: c_double) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Number(value));
}

unsafe extern "C" fn push_nil(lua_base: *mut RawLuaBase) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Nil);
}

unsafe extern "C" fn push_string(lua_base: *mut RawLuaBase, _: *const c_char, _: u32) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::GuardKey);
}

unsafe extern "C" fn push_bool(lua_base: *mut RawLuaBase, value: bool) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Bool(value));
}

unsafe extern "C" fn push_special(lua_base: *mut RawLuaBase, _: c_int) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Registry);
}

unsafe extern "C" fn raw_get(lua_base: *mut RawLuaBase, _: c_int) {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    assert_eq!(state.stack.pop(), Some(Value::GuardKey));
    state.stack.push(if state.guard_active {
        Value::Bool(true)
    } else {
        Value::Nil
    });
}

unsafe extern "C" fn raw_set(lua_base: *mut RawLuaBase, _: c_int) {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    state.guard_active = matches!(state.stack.pop(), Some(Value::Bool(true)));
    assert_eq!(state.stack.pop(), Some(Value::GuardKey));
}

unsafe extern "C" fn push_c_closure(
    lua_base: *mut RawLuaBase,
    callback: rsgdll_abi::LuaCFunction,
    _: c_int,
) {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    state.executor = Some(callback);
    state.stack.push(Value::Executor);
}

unsafe extern "C" fn reference_create(lua_base: *mut RawLuaBase) -> c_int {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.pop();
    1
}

unsafe extern "C" fn reference_free(_: *mut RawLuaBase, _: c_int) {}

unsafe extern "C" fn reference_push(lua_base: *mut RawLuaBase, _: c_int) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Executor);
}

unsafe extern "C" fn pcall(
    lua_base: *mut RawLuaBase,
    argument_count: c_int,
    result_count: c_int,
    _: c_int,
) -> c_int {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    let function_offset = state.stack.len() - argument_count as usize - 1;
    if state.stack.remove(function_offset) != Value::Executor {
        return 1;
    }
    let arguments = state.stack.split_off(function_offset);
    let outer = std::mem::replace(&mut state.stack, arguments);
    let Some(callback) = state.executor else {
        state.stack = outer;
        return 1;
    };
    let state_pointer = (state as *mut TestState).cast::<RawLuaState>();
    // SAFETY: callback receives this fixture's live state.
    let returned = unsafe { callback(state_pointer) };
    let returned = usize::try_from(returned).unwrap_or_default();
    let first_result = state.stack.len().saturating_sub(returned);
    let mut values = state.stack.split_off(first_result);
    state.stack = outer;
    if result_count >= 0 {
        values.resize(result_count as usize, Value::Nil);
        values.truncate(result_count as usize);
    }
    state.stack.extend(values);
    0
}

unsafe extern "C" fn get_type(lua_base: *mut RawLuaBase, index: c_int) -> c_int {
    if index == -10_003 {
        LuaType::NUMBER.0
    } else {
        // SAFETY: forwarded from matching fake vtable.
        let stack = &unsafe { state(lua_base) }.stack;
        let absolute = if index < 0 {
            stack.len().checked_add_signed(index as isize)
        } else {
            usize::try_from(index).ok()
        };
        match absolute.and_then(|index| stack.get(index)) {
            Some(Value::Nil) => LuaType::NIL.0,
            Some(Value::Number(_)) => LuaType::NUMBER.0,
            Some(Value::Bool(_)) => LuaType::BOOL.0,
            Some(Value::Executor) => LuaType::FUNCTION.0,
            Some(Value::Registry | Value::GuardKey) => LuaType::TABLE.0,
            None => LuaType::NONE.0,
        }
    }
}

unsafe extern "C" fn throw_error(lua_base: *mut RawLuaBase, message: *const c_char) {
    // SAFETY: bridge provides a NUL-terminated buffer valid for this call.
    let message = unsafe { std::ffi::CStr::from_ptr(message) };
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.error = Some(message.to_string_lossy().into_owned());
}

fn test_vtable() -> TestVTable {
    TestVTable {
        top,
        push: unused,
        pop,
        get_table: unused,
        get_field: unused,
        set_field: unused,
        create_table: unused,
        set_table: unused,
        set_meta_table: unused,
        get_meta_table: unused,
        call: unused,
        pcall,
        equal: unused,
        raw_equal: unused,
        insert: unused,
        remove: unused,
        next: unused,
        new_userdata: unused,
        throw_error,
        check_type: unused,
        arg_error: unused,
        raw_get,
        raw_set,
        get_string: unused,
        get_number,
        get_bool: unused,
        get_c_function: unused,
        get_userdata: unused,
        push_nil,
        push_string,
        push_number,
        push_bool,
        push_c_function: unused,
        push_c_closure,
        push_userdata: unused,
        reference_create,
        reference_free,
        reference_push,
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
        set_state: unused,
        create_meta_table: unused,
        push_meta_table: unused,
        push_user_type: unused,
        set_user_type: unused,
    }
}
