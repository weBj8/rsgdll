use core::ffi::{c_char, c_double, c_int, c_uint, c_void};
use core::mem::{offset_of, size_of};

use crate::{LuaCFunction, RawLuaState, SpecialIndex};

type RawVirtualSlot = unsafe extern "C" fn();

#[cfg(all(target_os = "windows", target_arch = "x86"))]
macro_rules! lua_method_fn {
    (($($argument:ty),* $(,)?) -> $return_type:ty) => {
        unsafe extern "thiscall" fn($($argument),*) -> $return_type
    };
}

#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
macro_rules! lua_method_fn {
    (($($argument:ty),* $(,)?) -> $return_type:ty) => {
        unsafe extern "C" fn($($argument),*) -> $return_type
    };
}

type VoidFn = lua_method_fn!((*mut RawLuaBase) -> ());
type TopFn = lua_method_fn!((*mut RawLuaBase) -> c_int);
type IntFn = lua_method_fn!((*mut RawLuaBase, c_int) -> ());
type IntResultFn = lua_method_fn!((*mut RawLuaBase, c_int) -> c_int);
type PCallFn = lua_method_fn!((*mut RawLuaBase, c_int, c_int, c_int) -> c_int);
type NewUserdataFn = lua_method_fn!((*mut RawLuaBase, c_uint) -> *mut c_void);
type GetUserdataFn = lua_method_fn!((*mut RawLuaBase, c_int) -> *mut c_void);
type CreateMetaTableFn = lua_method_fn!((*mut RawLuaBase, *const c_char) -> c_int);
type BoolIntFn = lua_method_fn!((*mut RawLuaBase, c_int) -> bool);
type SetUserTypeFn = lua_method_fn!((*mut RawLuaBase, c_int, *mut c_void) -> ());
type PushSpecialFn = lua_method_fn!((*mut RawLuaBase, SpecialIndex) -> ());
type GetStringFn = lua_method_fn!((*mut RawLuaBase, c_int, *mut c_uint) -> *const c_char);
type GetNumberFn = lua_method_fn!((*mut RawLuaBase, c_int) -> c_double);
type GetBoolFn = lua_method_fn!((*mut RawLuaBase, c_int) -> bool);
type PushStringFn = lua_method_fn!((*mut RawLuaBase, *const c_char, c_uint) -> ());
type PushNumberFn = lua_method_fn!((*mut RawLuaBase, c_double) -> ());
type PushBoolFn = lua_method_fn!((*mut RawLuaBase, bool) -> ());
type PushCClosureFn = lua_method_fn!((*mut RawLuaBase, LuaCFunction, c_int) -> ());
type ReferenceCreateFn = lua_method_fn!((*mut RawLuaBase) -> c_int);
type SetStateFn = lua_method_fn!((*mut RawLuaBase, *mut RawLuaState) -> ());

/// Raw C++ `GarrysMod::Lua::ILuaBase` object.
#[repr(C)]
pub struct RawLuaBase {
    vtable: *const RawLuaBaseVTable,
    state: *mut RawLuaState,
}

// Field order exactly follows LuaBase.h. Unexposed slots remain private
// function-pointer placeholders so callers cannot invoke arbitrary indices.
#[repr(C)]
struct RawLuaBaseVTable {
    top: TopFn,
    push: IntFn,
    pop: IntFn,
    get_table: RawVirtualSlot,
    get_field: RawVirtualSlot,
    set_field: RawVirtualSlot,
    create_table: VoidFn,
    set_table: RawVirtualSlot,
    set_meta_table: IntFn,
    get_meta_table: RawVirtualSlot,
    call: RawVirtualSlot,
    pcall: PCallFn,
    equal: RawVirtualSlot,
    raw_equal: RawVirtualSlot,
    insert: IntFn,
    remove: IntFn,
    next: IntResultFn,
    new_userdata: NewUserdataFn,
    throw_error: RawVirtualSlot,
    check_type: RawVirtualSlot,
    arg_error: RawVirtualSlot,
    raw_get: IntFn,
    raw_set: IntFn,
    get_string: GetStringFn,
    get_number: GetNumberFn,
    get_bool: GetBoolFn,
    get_c_function: RawVirtualSlot,
    get_userdata: GetUserdataFn,
    push_nil: VoidFn,
    push_string: PushStringFn,
    push_number: PushNumberFn,
    push_bool: PushBoolFn,
    push_c_function: RawVirtualSlot,
    push_c_closure: PushCClosureFn,
    push_userdata: RawVirtualSlot,
    reference_create: ReferenceCreateFn,
    reference_free: IntFn,
    reference_push: IntFn,
    push_special: PushSpecialFn,
    is_type: RawVirtualSlot,
    get_type: IntResultFn,
    get_type_name: RawVirtualSlot,
    create_meta_table_type: RawVirtualSlot,
    check_string: RawVirtualSlot,
    check_number: RawVirtualSlot,
    obj_len: RawVirtualSlot,
    get_angle: RawVirtualSlot,
    get_vector: RawVirtualSlot,
    push_angle: RawVirtualSlot,
    push_vector: RawVirtualSlot,
    set_state: SetStateFn,
    create_meta_table: CreateMetaTableFn,
    push_meta_table: BoolIntFn,
    push_user_type: RawVirtualSlot,
    set_user_type: SetUserTypeFn,
}

macro_rules! define_vtable_slots {
    ($($symbol:ident => $field:ident;)+) => {
        $(
            #[doc(hidden)]
            pub const $symbol: usize =
                offset_of!(RawLuaBaseVTable, $field) / size_of::<RawVirtualSlot>();
        )+
    };
}

define_vtable_slots! {
    RSGDLL_ABI_TOP_SLOT => top;
    RSGDLL_ABI_PUSH_SLOT => push;
    RSGDLL_ABI_POP_SLOT => pop;
    RSGDLL_ABI_CREATE_TABLE_SLOT => create_table;
    RSGDLL_ABI_SET_META_TABLE_SLOT => set_meta_table;
    RSGDLL_ABI_PCALL_SLOT => pcall;
    RSGDLL_ABI_REMOVE_SLOT => remove;
    RSGDLL_ABI_NEXT_SLOT => next;
    RSGDLL_ABI_NEW_USERDATA_SLOT => new_userdata;
    RSGDLL_ABI_THROW_ERROR_SLOT => throw_error;
    RSGDLL_ABI_RAW_GET_SLOT => raw_get;
    RSGDLL_ABI_RAW_SET_SLOT => raw_set;
    RSGDLL_ABI_PUSH_NIL_SLOT => push_nil;
    RSGDLL_ABI_PUSH_STRING_SLOT => push_string;
    RSGDLL_ABI_PUSH_NUMBER_SLOT => push_number;
    RSGDLL_ABI_PUSH_BOOL_SLOT => push_bool;
    RSGDLL_ABI_PUSH_CLOSURE_SLOT => push_c_closure;
    RSGDLL_ABI_REFERENCE_CREATE_SLOT => reference_create;
    RSGDLL_ABI_REFERENCE_FREE_SLOT => reference_free;
    RSGDLL_ABI_REFERENCE_PUSH_SLOT => reference_push;
    RSGDLL_ABI_PUSH_SPECIAL_SLOT => push_special;
    RSGDLL_ABI_GET_TYPE_SLOT => get_type;
    RSGDLL_ABI_SET_STATE_SLOT => set_state;
    RSGDLL_ABI_CREATE_META_TABLE_SLOT => create_meta_table;
    RSGDLL_ABI_PUSH_META_TABLE_SLOT => push_meta_table;
    RSGDLL_ABI_SET_USER_TYPE_SLOT => set_user_type;
}

macro_rules! raw_virtual_methods {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($argument:ident: $argument_type:ty),* $(,)?) -> $return_type:ty
            => $slot:ident;
    )+) => {
        impl RawLuaBase {
            $(
                $(#[$meta])*
                ///
                /// # Safety
                ///
                /// `lua_base` must be a live `ILuaBase` object using the pinned
                /// selected target vtable. All pointers and stack indices must
                /// satisfy the corresponding upstream method contract. The
                /// caller must prevent Lua longjmp, C++ exceptions, and Rust
                /// panics from crossing this invocation.
                #[inline]
                pub unsafe fn $name(
                    lua_base: *mut Self,
                    $($argument: $argument_type),*
                ) -> $return_type {
                    // SAFETY: [UB categories 3, 6, 8] Caller guarantees a live,
                    // aligned C++ object with the pinned layout.
                    let vtable = unsafe { (*lua_base).vtable };
                    // SAFETY: [UB category 8] The pinned header fixes this
                    // field's vtable position and exact function signature.
                    let function = unsafe { (*vtable).$slot };
                    // SAFETY: [UB categories 8, 14] Caller upholds the foreign
                    // method contract; no Rust-owned value is retained here.
                    unsafe { function(lua_base, $($argument),*) }
                }
            )+
        }
    };
}

raw_virtual_methods! {
    /// Returns current Lua stack height.
    fn top() -> c_int => top;
    /// Pushes a copy of one stack value.
    fn push(stack_index: c_int) -> () => push;
    /// Pops values from the stack.
    fn pop(count: c_int) -> () => pop;
    /// Creates and pushes a table.
    fn create_table() -> () => create_table;
    /// Calls one Lua function through Lua's protected-call boundary.
    fn pcall(argument_count: c_int, result_count: c_int, error_function: c_int) -> c_int
        => pcall;
    /// Sets the metatable of one stack value from the table at stack top.
    fn set_meta_table(stack_index: c_int) -> () => set_meta_table;
    /// Allocates and pushes full userdata storage.
    fn new_userdata(size: c_uint) -> *mut c_void => new_userdata;
    /// Gets a table value without invoking metamethods.
    fn raw_get(stack_index: c_int) -> () => raw_get;
    /// Sets a table value without invoking metamethods.
    fn raw_set(stack_index: c_int) -> () => raw_set;
    /// Advances table iteration using the key at stack top.
    fn next(stack_index: c_int) -> c_int => next;
    /// Returns a string pointer and optional byte length.
    fn get_string(stack_index: c_int, output_length: *mut c_uint) -> *const c_char
        => get_string;
    /// Returns a numeric value.
    fn get_number(stack_index: c_int) -> c_double => get_number;
    /// Returns a boolean value.
    fn get_bool(stack_index: c_int) -> bool => get_bool;
    /// Returns one full userdata header pointer.
    fn get_userdata(stack_index: c_int) -> *mut c_void => get_userdata;
    /// Pushes nil.
    fn push_nil() -> () => push_nil;
    /// Pushes string bytes; zero length delegates to upstream `strlen`.
    fn push_string(value: *const c_char, length: c_uint) -> () => push_string;
    /// Pushes a number.
    fn push_number(value: c_double) -> () => push_number;
    /// Pushes a boolean.
    fn push_bool(value: bool) -> () => push_bool;
    /// Pushes a Lua C closure.
    fn push_c_closure(value: LuaCFunction, upvalue_count: c_int) -> () => push_c_closure;
    /// Creates a registry reference from the stack's top value, consuming it.
    fn reference_create() -> c_int => reference_create;
    /// Releases one registry reference.
    fn reference_free(reference: c_int) -> () => reference_free;
    /// Pushes one registry-referenced value.
    fn reference_push(reference: c_int) -> () => reference_push;
    /// Pushes one special table.
    fn push_special(index: SpecialIndex) -> () => push_special;
    /// Gets one stack value's raw type tag.
    fn get_type(stack_index: c_int) -> c_int => get_type;
    /// Sets the state used by inline `ILuaBase` helpers at callback entry.
    fn set_state(state: *mut RawLuaState) -> () => set_state;
    /// Creates or retrieves a named metatable and pushes it.
    fn create_meta_table(name: *const c_char) -> c_int => create_meta_table;
    /// Pushes a metatable by type identifier.
    fn push_meta_table(lua_type: c_int) -> bool => push_meta_table;
    /// Invalidates or replaces one userdata data pointer.
    fn set_user_type(stack_index: c_int, data: *mut c_void) -> () => set_user_type;
}

const _: () = assert!(core::mem::size_of::<RawVirtualSlot>() == core::mem::size_of::<*const ()>());
const _: () = assert!(core::mem::size_of::<RawLuaBase>() == 2 * core::mem::size_of::<*const ()>());
const _: () = assert!(core::mem::align_of::<RawLuaBase>() == core::mem::align_of::<*const ()>());
const _: () =
    assert!(core::mem::offset_of!(RawLuaBase, state) == core::mem::size_of::<*const ()>());

#[cfg(test)]
mod tests;
