use std::{ffi::c_void, ptr::NonNull};

use rsgdll_bridge::{
    LuaOperation, OP_CREATE_META_TABLE, OP_CREATE_TABLE, OP_NEW_USERDATA, OP_NEXT, OP_PCALL,
    OP_POP, OP_PUSH, OP_PUSH_BOOL, OP_PUSH_C_CLOSURE, OP_PUSH_META_TABLE, OP_PUSH_NIL,
    OP_PUSH_NUMBER, OP_PUSH_STRING, OP_RAW_GET, OP_RAW_SET, OP_REFERENCE_CREATE, OP_REFERENCE_FREE,
    OP_REFERENCE_PUSH, OP_SET_META_TABLE, OP_SET_USER_TYPE,
};
use rsgdll_platform::__private::{LuaCFunction, RawLuaBase, RawLuaState};

use crate::{LuaError, LuaResult};

#[derive(Clone, Copy)]
pub(crate) struct Context {
    state: NonNull<RawLuaState>,
    raw: NonNull<RawLuaBase>,
}

impl Context {
    pub(crate) const fn new(state: NonNull<RawLuaState>, raw: NonNull<RawLuaBase>) -> Self {
        Self { state, raw }
    }
}

fn execute(context: Context, operation: &mut LuaOperation) -> LuaResult<()> {
    // SAFETY: `Lua::from_raw` recorded the live state and matching `ILuaBase`.
    // Each operation builder keeps any pointer argument live for this call.
    let status = unsafe {
        rsgdll_bridge::execute(
            context.state.as_ptr(),
            context.raw.as_ptr().cast::<c_void>(),
            operation,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(LuaError::ProtectedOperation)
    }
}

pub(crate) fn push(context: Context, index: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_PUSH);
    operation.a = index;
    execute(context, &mut operation)
}

pub(crate) fn pop(context: Context, count: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_POP);
    operation.a = count;
    execute(context, &mut operation)
}

pub(crate) fn create_table(context: Context) -> LuaResult<()> {
    execute(context, &mut LuaOperation::new(OP_CREATE_TABLE))
}

pub(crate) fn pcall(context: Context, argument_count: i32, result_count: i32) -> LuaResult<i32> {
    let mut operation = LuaOperation::new(OP_PCALL);
    operation.a = argument_count;
    operation.b = result_count;
    execute(context, &mut operation)?;
    i32::try_from(operation.result_integer).map_err(|_| LuaError::CountOverflow)
}

pub(crate) fn set_meta_table(context: Context, index: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_SET_META_TABLE);
    operation.a = index;
    execute(context, &mut operation)
}

pub(crate) fn new_userdata(context: Context, size: u32) -> LuaResult<*mut c_void> {
    let mut operation = LuaOperation::new(OP_NEW_USERDATA);
    operation.length = size;
    execute(context, &mut operation)?;
    Ok(operation.result_pointer)
}

pub(crate) fn raw_get(context: Context, index: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_RAW_GET);
    operation.a = index;
    execute(context, &mut operation)
}

pub(crate) fn raw_set(context: Context, index: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_RAW_SET);
    operation.a = index;
    execute(context, &mut operation)
}

pub(crate) fn next(context: Context, index: i32) -> LuaResult<bool> {
    let mut operation = LuaOperation::new(OP_NEXT);
    operation.a = index;
    execute(context, &mut operation)?;
    Ok(operation.result_integer != 0)
}

pub(crate) fn push_nil(context: Context) -> LuaResult<()> {
    execute(context, &mut LuaOperation::new(OP_PUSH_NIL))
}

pub(crate) fn push_string(context: Context, value: *const u8, length: u32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_PUSH_STRING);
    operation.pointer = value.cast::<c_void>();
    operation.length = length;
    execute(context, &mut operation)
}

pub(crate) fn push_number(context: Context, value: f64) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_PUSH_NUMBER);
    operation.number = value;
    execute(context, &mut operation)
}

pub(crate) fn push_bool(context: Context, value: bool) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_PUSH_BOOL);
    operation.a = i32::from(value);
    execute(context, &mut operation)
}

pub(crate) fn push_c_closure(
    context: Context,
    function: LuaCFunction,
    upvalue_count: i32,
) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_PUSH_C_CLOSURE);
    operation.pointer = function as *const c_void;
    operation.a = upvalue_count;
    execute(context, &mut operation)
}

pub(crate) fn reference_create(context: Context) -> LuaResult<i32> {
    let mut operation = LuaOperation::new(OP_REFERENCE_CREATE);
    execute(context, &mut operation)?;
    i32::try_from(operation.result_integer).map_err(|_| LuaError::CountOverflow)
}

pub(crate) fn reference_free(context: Context, id: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_REFERENCE_FREE);
    operation.a = id;
    execute(context, &mut operation)
}

pub(crate) fn reference_push(context: Context, id: i32) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_REFERENCE_PUSH);
    operation.a = id;
    execute(context, &mut operation)
}

pub(crate) fn create_meta_table(context: Context, name: *const u8) -> LuaResult<i32> {
    let mut operation = LuaOperation::new(OP_CREATE_META_TABLE);
    operation.pointer = name.cast::<c_void>();
    execute(context, &mut operation)?;
    i32::try_from(operation.result_integer).map_err(|_| LuaError::CountOverflow)
}

pub(crate) fn push_meta_table(context: Context, lua_type: i32) -> LuaResult<bool> {
    let mut operation = LuaOperation::new(OP_PUSH_META_TABLE);
    operation.a = lua_type;
    execute(context, &mut operation)?;
    Ok(operation.result_integer != 0)
}

pub(crate) fn set_user_type(context: Context, index: i32, data: *mut c_void) -> LuaResult<()> {
    let mut operation = LuaOperation::new(OP_SET_USER_TYPE);
    operation.a = index;
    operation.pointer = data.cast_const();
    execute(context, &mut operation)
}
