use core::ffi::{c_char, c_double, c_int, c_uint};

use crate::{LuaCFunction, RawLuaState, SpecialIndex};

type RawVirtualSlot = unsafe extern "C" fn();
type VoidFn = unsafe extern "C" fn(*mut RawLuaBase);
type TopFn = unsafe extern "C" fn(*mut RawLuaBase) -> c_int;
type IntFn = unsafe extern "C" fn(*mut RawLuaBase, c_int);
type IntResultFn = unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_int;
type PushSpecialFn = unsafe extern "C" fn(*mut RawLuaBase, SpecialIndex);
type GetStringFn = unsafe extern "C" fn(*mut RawLuaBase, c_int, *mut c_uint) -> *const c_char;
type GetNumberFn = unsafe extern "C" fn(*mut RawLuaBase, c_int) -> c_double;
type GetBoolFn = unsafe extern "C" fn(*mut RawLuaBase, c_int) -> bool;
type PushStringFn = unsafe extern "C" fn(*mut RawLuaBase, *const c_char, c_uint);
type PushNumberFn = unsafe extern "C" fn(*mut RawLuaBase, c_double);
type PushBoolFn = unsafe extern "C" fn(*mut RawLuaBase, bool);
type PushCClosureFn = unsafe extern "C" fn(*mut RawLuaBase, LuaCFunction, c_int);
type SetStateFn = unsafe extern "C" fn(*mut RawLuaBase, *mut RawLuaState);

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
    set_meta_table: RawVirtualSlot,
    get_meta_table: RawVirtualSlot,
    call: RawVirtualSlot,
    pcall: RawVirtualSlot,
    equal: RawVirtualSlot,
    raw_equal: RawVirtualSlot,
    insert: IntFn,
    remove: IntFn,
    next: IntResultFn,
    new_userdata: RawVirtualSlot,
    throw_error: RawVirtualSlot,
    check_type: RawVirtualSlot,
    arg_error: RawVirtualSlot,
    raw_get: IntFn,
    raw_set: IntFn,
    get_string: GetStringFn,
    get_number: GetNumberFn,
    get_bool: GetBoolFn,
    get_c_function: RawVirtualSlot,
    get_userdata: RawVirtualSlot,
    push_nil: VoidFn,
    push_string: PushStringFn,
    push_number: PushNumberFn,
    push_bool: PushBoolFn,
    push_c_function: RawVirtualSlot,
    push_c_closure: PushCClosureFn,
    push_userdata: RawVirtualSlot,
    reference_create: RawVirtualSlot,
    reference_free: RawVirtualSlot,
    reference_push: RawVirtualSlot,
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
    create_meta_table: RawVirtualSlot,
    push_meta_table: RawVirtualSlot,
    push_user_type: RawVirtualSlot,
    set_user_type: RawVirtualSlot,
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
                /// Linux x86_64 vtable. All pointers and stack indices must
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
    /// Gets a table value without invoking metamethods.
    fn raw_get(stack_index: c_int) -> () => raw_get;
    /// Sets a table value without invoking metamethods.
    fn raw_set(stack_index: c_int) -> () => raw_set;
    /// Returns a string pointer and optional byte length.
    fn get_string(stack_index: c_int, output_length: *mut c_uint) -> *const c_char
        => get_string;
    /// Returns a numeric value.
    fn get_number(stack_index: c_int) -> c_double => get_number;
    /// Returns a boolean value.
    fn get_bool(stack_index: c_int) -> bool => get_bool;
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
    /// Pushes one special table.
    fn push_special(index: SpecialIndex) -> () => push_special;
    /// Gets one stack value's raw type tag.
    fn get_type(stack_index: c_int) -> c_int => get_type;
    /// Sets the state used by inline `ILuaBase` helpers at callback entry.
    fn set_state(state: *mut RawLuaState) -> () => set_state;
}

const _: () = assert!(core::mem::size_of::<RawVirtualSlot>() == core::mem::size_of::<*const ()>());
const _: () = assert!(core::mem::size_of::<RawLuaBase>() == 16);
const _: () = assert!(core::mem::align_of::<RawLuaBase>() == 8);
const _: () = assert!(core::mem::offset_of!(RawLuaBase, state) == 8);

#[cfg(test)]
mod tests;
