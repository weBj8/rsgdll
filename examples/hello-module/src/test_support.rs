use std::collections::HashMap;
use std::ffi::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr::NonNull;

type LuaFunction = unsafe extern "C" fn(*mut c_void) -> c_int;
type Slot = *const c_void;

#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Table(HashMap<Vec<u8>, Value>),
    Function {
        callback: LuaFunction,
        upvalues: Vec<Value>,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::Number(left), Self::Number(right)) => left == right,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Table(left), Self::Table(right)) => left == right,
            (
                Self::Function {
                    callback: left_callback,
                    upvalues: left_upvalues,
                },
                Self::Function {
                    callback: right_callback,
                    upvalues: right_upvalues,
                },
            ) => {
                std::ptr::fn_addr_eq(*left_callback, *right_callback)
                    && left_upvalues == right_upvalues
            }
            _ => false,
        }
    }
}

#[repr(C)]
struct TestState {
    prefix: [u8; 120],
    lua_base: *mut TestLuaBase,
    stack: Vec<Value>,
    upvalues: Vec<Value>,
    error: Option<String>,
}

#[repr(C)]
struct TestLuaBase {
    vtable: *const Slot,
    state: *mut TestState,
}

pub struct Fixture {
    state: NonNull<TestState>,
    lua_base: NonNull<TestLuaBase>,
    vtable: NonNull<[Slot; 55]>,
    functions: HashMap<Vec<u8>, Value>,
}

impl Fixture {
    pub fn new() -> Self {
        let vtable = NonNull::from(Box::leak(Box::new(test_vtable())));
        let lua_base = NonNull::from(Box::leak(Box::new(TestLuaBase {
            vtable: vtable.as_ptr().cast(),
            state: std::ptr::null_mut(),
        })));
        let state = NonNull::from(Box::leak(Box::new(TestState {
            prefix: [0; 120],
            lua_base: lua_base.as_ptr(),
            stack: Vec::new(),
            upvalues: Vec::new(),
            error: None,
        })));
        Self {
            state,
            lua_base,
            vtable,
            functions: HashMap::new(),
        }
    }

    pub fn state(&mut self) -> *mut c_void {
        self.state.as_ptr().cast()
    }

    pub fn call(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
    ) -> (c_int, Vec<Value>, Option<String>) {
        // SAFETY: fixture exclusively owns live state.
        let state = unsafe { self.state.as_mut() };
        if let Some(Value::Table(table)) = state.stack.first() {
            self.functions = table.clone();
        }
        let function = self
            .functions
            .get(name.as_bytes())
            .cloned()
            .unwrap_or_else(|| panic!("registered Lua function `{name}`"));
        let Value::Function { callback, upvalues } = function else {
            panic!("registered value is not a function");
        };
        state.stack = arguments;
        state.upvalues = upvalues;
        state.error = None;
        // SAFETY: generated closure receives its matching live fake state.
        let count = unsafe { callback(self.state()) };
        let output_count = usize::try_from(count).expect("nonnegative return count");
        let outputs = state.stack[state.stack.len() - output_count..].to_vec();
        (count, outputs, state.error.clone())
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

unsafe fn state<'a>(lua_base: *mut c_void) -> &'a mut TestState {
    // SAFETY: fake vtable receives only its matching live object.
    unsafe { &mut *(*lua_base.cast::<TestLuaBase>()).state }
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

unsafe extern "C" fn unused() {}

unsafe extern "C" fn top(lua_base: *mut c_void) -> c_int {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.len() as c_int
}

unsafe extern "C" fn pop(lua_base: *mut c_void, count: c_int) {
    // SAFETY: framework pops only observed stack values.
    let state = unsafe { state(lua_base) };
    state.stack.truncate(state.stack.len() - count as usize);
}

unsafe extern "C" fn create_table(lua_base: *mut c_void) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }
        .stack
        .push(Value::Table(HashMap::new()));
}

unsafe extern "C" fn raw_set(lua_base: *mut c_void, index: c_int) {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    let table = stack_offset(state, index);
    let assigned = state.stack.pop();
    let key = state.stack.pop();
    if let (Some(table), Some(Value::String(key)), Some(assigned)) = (table, key, assigned)
        && let Some(Value::Table(values)) = state.stack.get_mut(table)
    {
        values.insert(key, assigned);
    }
}

unsafe extern "C" fn throw_error(lua_base: *mut c_void, message: *const c_char) {
    // SAFETY: bridge supplies a NUL-terminated message for this call.
    let message = unsafe { std::ffi::CStr::from_ptr(message) };
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.error = Some(message.to_string_lossy().into_owned());
}

unsafe extern "C" fn get_string(
    lua_base: *mut c_void,
    index: c_int,
    output_length: *mut c_uint,
) -> *const c_char {
    // SAFETY: forwarded from matching fake vtable.
    let Some(Value::String(bytes)) = value(unsafe { state(lua_base) }, index) else {
        return std::ptr::null();
    };
    // SAFETY: framework supplies writable length storage.
    unsafe { output_length.write(bytes.len() as c_uint) };
    bytes.as_ptr().cast()
}

unsafe extern "C" fn get_number(lua_base: *mut c_void, index: c_int) -> c_double {
    // SAFETY: forwarded from matching fake vtable.
    match value(unsafe { state(lua_base) }, index) {
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    }
}

unsafe extern "C" fn get_bool(lua_base: *mut c_void, index: c_int) -> bool {
    // SAFETY: forwarded from matching fake vtable.
    matches!(
        value(unsafe { state(lua_base) }, index),
        Some(Value::Bool(true))
    )
}

unsafe extern "C" fn push_nil(lua_base: *mut c_void) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Nil);
}

unsafe extern "C" fn push_string(lua_base: *mut c_void, bytes: *const c_char, length: c_uint) {
    // SAFETY: bridge supplies a readable buffer of exactly `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(bytes.cast(), length as usize) };
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }
        .stack
        .push(Value::String(bytes.to_vec()));
}

unsafe extern "C" fn push_number(lua_base: *mut c_void, value: c_double) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Number(value));
}

unsafe extern "C" fn push_bool(lua_base: *mut c_void, value: bool) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { state(lua_base) }.stack.push(Value::Bool(value));
}

unsafe extern "C" fn push_closure(
    lua_base: *mut c_void,
    callback: LuaFunction,
    upvalue_count: c_int,
) {
    // SAFETY: forwarded from matching fake vtable.
    let state = unsafe { state(lua_base) };
    let first = state.stack.len() - upvalue_count as usize;
    let upvalues = state.stack.split_off(first);
    state.stack.push(Value::Function { callback, upvalues });
}

unsafe extern "C" fn get_type(lua_base: *mut c_void, index: c_int) -> c_int {
    // SAFETY: forwarded from matching fake vtable.
    match value(unsafe { state(lua_base) }, index) {
        None => -1,
        Some(Value::Nil) => 0,
        Some(Value::Bool(_)) => 1,
        Some(Value::Number(_)) => 3,
        Some(Value::String(_)) => 4,
        Some(Value::Table(_)) => 5,
        Some(Value::Function { .. }) => 6,
    }
}

unsafe extern "C" fn set_state(lua_base: *mut c_void, state: *mut c_void) {
    // SAFETY: forwarded from matching fake vtable.
    unsafe { (*lua_base.cast::<TestLuaBase>()).state = state.cast() };
}

fn slot(function: *const ()) -> Slot {
    function.cast()
}

fn test_vtable() -> [Slot; 55] {
    let mut slots = [slot(unused as *const ()); 55];
    slots[0] = slot(top as *const ());
    slots[2] = slot(pop as *const ());
    slots[6] = slot(create_table as *const ());
    slots[18] = slot(throw_error as *const ());
    slots[22] = slot(raw_set as *const ());
    slots[23] = slot(get_string as *const ());
    slots[24] = slot(get_number as *const ());
    slots[25] = slot(get_bool as *const ());
    slots[28] = slot(push_nil as *const ());
    slots[29] = slot(push_string as *const ());
    slots[30] = slot(push_number as *const ());
    slots[31] = slot(push_bool as *const ());
    slots[33] = slot(push_closure as *const ());
    slots[40] = slot(get_type as *const ());
    slots[50] = slot(set_state as *const ());
    slots
}
