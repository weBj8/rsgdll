use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_double, c_int, c_uint, c_void};
use std::ptr::NonNull;
use std::rc::Rc;

use rsgdll_abi::{LuaCFunction, LuaType, RawLuaBase, RawLuaState, RawUserData, SpecialIndex};
use rsgdll_lua::{Lua, LuaResult};

pub trait LuaTestExt: Sized {
    /// Constructs a checked handle from this fixture's callback state.
    ///
    /// # Safety
    ///
    /// The state must remain live and exclusively main-thread-bound.
    unsafe fn from_raw(state: *mut RawLuaState) -> LuaResult<Self>;
}

impl<'lua> LuaTestExt for Lua<'lua> {
    unsafe fn from_raw(state: *mut RawLuaState) -> LuaResult<Self> {
        // SAFETY: caller upholds the fixture state contract.
        unsafe { rsgdll_lua::__private::from_raw(state) }
    }
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Table(Rc<RefCell<HashMap<Key, Value>>>),
    Function,
    FunctionCallback {
        callback: LuaCFunction,
        upvalues: Vec<Value>,
    },
    FunctionReturns(Vec<Value>),
    FunctionError(Vec<u8>),
    UserData(*mut RawUserData),
    GenericUserData(*mut c_void),
    Entity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    String(Vec<u8>),
    Number(u64),
}

#[repr(C)]
struct TestState {
    prefix: [u8; 120],
    lua_base: *mut RawLuaBase,
    stack: Vec<Value>,
    upvalues: Vec<Value>,
    references: HashMap<c_int, Value>,
    next_reference: c_int,
    metatable_names: HashMap<Vec<u8>, c_int>,
    metatables: HashMap<c_int, Value>,
    next_userdata_type: c_int,
    userdata_allocations: Vec<*mut RawUserData>,
    get_userdata_calls: usize,
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
    set_meta_table: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_meta_table: Slot,
    call: Slot,
    pcall: unsafe extern "C" fn(*mut RawLuaBase, c_int, c_int, c_int) -> c_int,
    equal: Slot,
    raw_equal: Slot,
    insert: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    remove: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    next: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_int,
    new_userdata: unsafe extern "C" fn(*mut RawLuaBase, c_uint) -> *mut c_void,
    throw_error: Slot,
    check_type: Slot,
    arg_error: Slot,
    raw_get: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    raw_set: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    get_string: unsafe extern "C" fn(*mut RawLuaBase, c_int, *mut c_uint) -> *const c_char,
    get_number: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_double,
    get_bool: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> bool,
    get_c_function: Slot,
    get_userdata: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> *mut c_void,
    push_nil: unsafe extern "C" fn(*mut RawLuaBase),
    push_string: unsafe extern "C" fn(*mut RawLuaBase, *const c_char, c_uint),
    push_number: unsafe extern "C" fn(*mut RawLuaBase, c_double),
    push_bool: unsafe extern "C" fn(*mut RawLuaBase, bool),
    push_c_function: Slot,
    push_c_closure: unsafe extern "C" fn(*mut RawLuaBase, LuaCFunction, c_int),
    push_userdata: Slot,
    reference_create: unsafe extern "C" fn(*mut RawLuaBase) -> c_int,
    reference_free: unsafe extern "C" fn(*mut RawLuaBase, c_int),
    reference_push: unsafe extern "C" fn(*mut RawLuaBase, c_int),
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
    create_meta_table: unsafe extern "C" fn(*mut RawLuaBase, *const c_char) -> c_int,
    push_meta_table: unsafe extern "C" fn(*mut RawLuaBase, c_int) -> bool,
    push_user_type: Slot,
    set_user_type: unsafe extern "C" fn(*mut RawLuaBase, c_int, *mut c_void),
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
            references: HashMap::new(),
            next_reference: 1,
            metatable_names: HashMap::new(),
            metatables: HashMap::new(),
            next_userdata_type: 1,
            userdata_allocations: Vec::new(),
            get_userdata_calls: 0,
        })));
        // SAFETY: raw-owned allocations remain live until `Fixture::drop`.
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

    pub fn top(&self) -> usize {
        // SAFETY: allocation remains live and callers inspect only when no
        // `Lua` access is active.
        unsafe { (*self.state.as_ptr()).stack.len() }
    }

    pub fn reference_count(&self) -> usize {
        // SAFETY: allocation remains live and callers inspect only when no
        // `Lua` access is active.
        unsafe { (*self.state.as_ptr()).references.len() }
    }

    pub fn get_userdata_calls(&self) -> usize {
        // SAFETY: allocation remains live and callers inspect only when no
        // `Lua` access is active.
        unsafe { (*self.state.as_ptr()).get_userdata_calls }
    }

    pub fn push_foreign_userdata(&mut self, lua_type: u8, data: *mut c_void) {
        let header = Box::into_raw(Box::new(RawUserData { data, lua_type }));
        // SAFETY: fixture exclusively owns the live test state.
        let state = unsafe { self.state.as_mut() };
        state.userdata_allocations.push(header);
        state.stack.push(Value::UserData(header));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: each pointer came from one Box::into_raw in `new_userdata`.
        for allocation in unsafe { &mut (*self.state.as_ptr()).userdata_allocations }.drain(..) {
            // SAFETY: each allocation is reclaimed exactly once here.
            unsafe { drop(Box::from_raw(allocation)) };
        }
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

fn table_key(value: Value) -> Option<Key> {
    match value {
        Value::String(value) => Some(Key::String(value)),
        Value::Number(value) => Some(Key::Number(value.to_bits())),
        _ => None,
    }
}

fn key_value(key: Key) -> Value {
    match key {
        Key::String(value) => Value::String(value),
        Key::Number(value) => Value::Number(f64::from_bits(value)),
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

unsafe extern "C" fn insert(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let Some(offset) = stack_offset(state, index) else {
        return;
    };
    if let Some(value) = state.stack.pop() {
        state.stack.insert(offset, value);
    }
}

unsafe extern "C" fn remove(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    if let Some(offset) = stack_offset(state, index) {
        state.stack.remove(offset);
    }
}

unsafe extern "C" fn create_table(lua_base: *mut RawLuaBase) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .stack
        .push(Value::Table(Rc::new(RefCell::new(HashMap::new()))));
}

unsafe extern "C" fn raw_get(lua_base: *mut RawLuaBase, index: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let table_offset = stack_offset(state, index);
    let key = state.stack.pop();
    let result = match (table_offset, key.and_then(table_key)) {
        (Some(offset), Some(key)) => match state.stack.get(offset) {
            Some(Value::Table(table)) => table.borrow().get(&key).cloned().unwrap_or(Value::Nil),
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
    let key = state.stack.pop().and_then(table_key);
    if let (Some(offset), Some(key), Some(assigned)) = (table_offset, key, assigned)
        && let Some(Value::Table(table)) = state.stack.get(offset)
    {
        table.borrow_mut().insert(key, assigned);
    }
}

unsafe extern "C" fn next(lua_base: *mut RawLuaBase, index: c_int) -> c_int {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let table_offset = stack_offset(state, index);
    let current = state.stack.pop();
    let Some(Value::Table(table)) = table_offset
        .and_then(|offset| state.stack.get(offset))
        .cloned()
    else {
        return 0;
    };
    let table = table.borrow();
    let mut keys = table.keys().cloned().collect::<Vec<_>>();
    keys.sort_unstable();
    let next_key = match current {
        Some(Value::Nil) => keys.first().cloned(),
        Some(current) => table_key(current).and_then(|current| {
            keys.iter()
                .position(|key| key == &current)
                .and_then(|offset| keys.get(offset + 1))
                .cloned()
        }),
        _ => None,
    };
    let Some(key) = next_key else {
        return 0;
    };
    let value = table.get(&key).cloned().unwrap_or(Value::Nil);
    state.stack.push(key_value(key));
    state.stack.push(value);
    1
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
    callback: LuaCFunction,
    upvalue_count: c_int,
) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let first_upvalue = state.stack.len() - upvalue_count as usize;
    let upvalues = state.stack.split_off(first_upvalue);
    state
        .stack
        .push(Value::FunctionCallback { callback, upvalues });
}

unsafe extern "C" fn pcall(
    lua_base: *mut RawLuaBase,
    argument_count: c_int,
    result_count: c_int,
    _: c_int,
) -> c_int {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let function_offset = state.stack.len() - argument_count as usize - 1;
    let function = state.stack.remove(function_offset);
    let arguments = state.stack.split_off(function_offset);
    match function {
        Value::Function | Value::FunctionReturns(_) if result_count == 0 => 0,
        Value::FunctionReturns(mut values) => {
            values.resize(result_count as usize, Value::Nil);
            values.truncate(result_count as usize);
            state.stack.extend(values);
            0
        }
        Value::FunctionError(message) => {
            state.stack.push(Value::String(message));
            1
        }
        Value::FunctionCallback { callback, upvalues } => {
            let outer_stack = std::mem::replace(&mut state.stack, arguments);
            let outer_upvalues = std::mem::replace(&mut state.upvalues, upvalues);
            let state_pointer = (state as *mut TestState).cast::<RawLuaState>();
            // SAFETY: callback receives this fixture's live state.
            let returned = unsafe { callback(state_pointer) };
            let returned = usize::try_from(returned).unwrap_or_default();
            let first_result = state.stack.len().saturating_sub(returned);
            let mut values = state.stack.split_off(first_result);
            state.stack = outer_stack;
            state.upvalues = outer_upvalues;
            if result_count >= 0 {
                values.resize(result_count as usize, Value::Nil);
                values.truncate(result_count as usize);
            }
            state.stack.extend(values);
            0
        }
        _ => {
            state
                .stack
                .push(Value::String(b"attempt to call non-function".to_vec()));
            1
        }
    }
}

unsafe extern "C" fn new_userdata(lua_base: *mut RawLuaBase, _: c_uint) -> *mut c_void {
    let header = Box::into_raw(Box::new(RawUserData {
        data: std::ptr::null_mut(),
        lua_type: 0,
    }));
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    state.userdata_allocations.push(header);
    state.stack.push(Value::UserData(header));
    header.cast()
}

unsafe extern "C" fn get_userdata(lua_base: *mut RawLuaBase, index: c_int) -> *mut c_void {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    state.get_userdata_calls += 1;
    match value(state, index) {
        Some(Value::UserData(header)) => header.cast(),
        Some(Value::GenericUserData(pointer)) => *pointer,
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn set_meta_table(lua_base: *mut RawLuaBase, _: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }.stack.pop();
}

unsafe extern "C" fn create_meta_table(lua_base: *mut RawLuaBase, name: *const c_char) -> c_int {
    // SAFETY: wrapper supplies a live NUL-terminated name.
    let name = unsafe { CStr::from_ptr(name) }.to_bytes().to_vec();
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let lua_type = match state.metatable_names.get(&name) {
        Some(lua_type) => *lua_type,
        None => {
            let lua_type = state.next_userdata_type;
            state.next_userdata_type += 1;
            state.metatable_names.insert(name, lua_type);
            state.metatables.insert(
                lua_type,
                Value::Table(Rc::new(RefCell::new(HashMap::new()))),
            );
            lua_type
        }
    };
    if let Some(metatable) = state.metatables.get(&lua_type).cloned() {
        state.stack.push(metatable);
    }
    lua_type
}

unsafe extern "C" fn push_meta_table(lua_base: *mut RawLuaBase, lua_type: c_int) -> bool {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let Some(metatable) = state.metatables.get(&lua_type).cloned() else {
        return false;
    };
    state.stack.push(metatable);
    true
}

unsafe extern "C" fn set_user_type(lua_base: *mut RawLuaBase, index: c_int, data: *mut c_void) {
    // SAFETY: forwarded from a live fixture vtable.
    if let Some(Value::UserData(header)) = value(unsafe { test_state(lua_base) }, index) {
        // SAFETY: fixture allocated this writable header.
        unsafe { (**header).data = data };
    }
}

unsafe extern "C" fn reference_create(lua_base: *mut RawLuaBase) -> c_int {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    let reference = state.next_reference;
    state.next_reference += 1;
    if let Some(value) = state.stack.pop() {
        state.references.insert(reference, value);
    }
    reference
}

unsafe extern "C" fn reference_free(lua_base: *mut RawLuaBase, reference: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { test_state(lua_base) }
        .references
        .remove(&reference);
}

unsafe extern "C" fn reference_push(lua_base: *mut RawLuaBase, reference: c_int) {
    // SAFETY: forwarded from a live fixture vtable.
    let state = unsafe { test_state(lua_base) };
    if let Some(value) = state.references.get(&reference).cloned() {
        state.stack.push(value);
    }
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
        Some(
            Value::Function
            | Value::FunctionCallback { .. }
            | Value::FunctionReturns(_)
            | Value::FunctionError(_),
        ) => LuaType::FUNCTION.0,
        Some(Value::UserData(header)) => {
            // SAFETY: fixture owns this live userdata header.
            i32::from(unsafe { (**header).lua_type })
        }
        Some(Value::GenericUserData(_)) => LuaType::USER_DATA.0,
        Some(Value::Entity) => LuaType::ENTITY.0,
    }
}

unsafe extern "C" fn set_state(lua_base: *mut RawLuaBase, state: *mut RawLuaState) {
    // SAFETY: forwarded from a live fixture vtable.
    unsafe { (*(lua_base.cast::<TestLuaBase>())).state = state };
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
        set_meta_table,
        get_meta_table: unused,
        call: unused,
        pcall,
        equal: unused,
        raw_equal: unused,
        insert,
        remove,
        next,
        new_userdata,
        throw_error: unused,
        check_type: unused,
        arg_error: unused,
        raw_get,
        raw_set,
        get_string,
        get_number,
        get_bool,
        get_c_function: unused,
        get_userdata,
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
        set_state,
        create_meta_table,
        push_meta_table,
        push_user_type: unused,
        set_user_type,
    }
}
