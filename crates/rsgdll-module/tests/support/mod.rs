use std::ffi::{c_char, c_double, c_int};
use std::ptr::NonNull;

use rsgdll_abi::{LuaType, RawLuaBase, RawLuaState};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
}

#[repr(C)]
struct TestState {
    prefix: [u8; 120],
    lua_base: *mut RawLuaBase,
    stack: Vec<Value>,
    callback_id: f64,
    error: Option<String>,
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
    pcall: Slot,
    equal: Slot,
    raw_equal: Slot,
    insert: Slot,
    remove: Slot,
    next: Slot,
    new_userdata: Slot,
    throw_error: unsafe extern "C" fn(*mut RawLuaBase, *const c_char),
    check_type: Slot,
    arg_error: Slot,
    raw_get: Slot,
    raw_set: Slot,
    get_string: Slot,
    get_number: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_double,
    get_bool: Slot,
    get_c_function: Slot,
    get_userdata: Slot,
    push_nil: Slot,
    push_string: Slot,
    push_number: unsafe extern "C" fn(*mut RawLuaBase, c_double),
    push_bool: Slot,
    push_c_function: Slot,
    push_c_closure: Slot,
    push_userdata: Slot,
    reference_create: Slot,
    reference_free: Slot,
    reference_push: Slot,
    push_special: Slot,
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
        })));
        // SAFETY: all allocations remain live until `Fixture::drop`.
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

unsafe extern "C" fn get_type(_: *mut RawLuaBase, index: c_int) -> c_int {
    if index == -10_003 {
        LuaType::NUMBER.0
    } else {
        LuaType::NONE.0
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
        pcall: unused,
        equal: unused,
        raw_equal: unused,
        insert: unused,
        remove: unused,
        next: unused,
        new_userdata: unused,
        throw_error,
        check_type: unused,
        arg_error: unused,
        raw_get: unused,
        raw_set: unused,
        get_string: unused,
        get_number,
        get_bool: unused,
        get_c_function: unused,
        get_userdata: unused,
        push_nil: unused,
        push_string: unused,
        push_number,
        push_bool: unused,
        push_c_function: unused,
        push_c_closure: unused,
        push_userdata: unused,
        reference_create: unused,
        reference_free: unused,
        reference_push: unused,
        push_special: unused,
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
