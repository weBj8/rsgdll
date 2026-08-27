use core::sync::atomic::{AtomicBool, Ordering};

use super::*;

static PUSHED_BOOL: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn unused() {}

macro_rules! lua_methods {
    ($(fn $name:ident($($argument:tt: $argument_type:ty),* $(,)?) $(-> $return_type:ty)? $body:block)+) => {
        $(
            #[cfg(all(target_os = "windows", target_arch = "x86"))]
            unsafe extern "thiscall" fn $name(
                $($argument: $argument_type),*
            ) $(-> $return_type)? $body

            #[cfg(not(all(target_os = "windows", target_arch = "x86")))]
            unsafe extern "C" fn $name(
                $($argument: $argument_type),*
            ) $(-> $return_type)? $body
        )+
    };
}

lua_methods! {
    fn void(_: *mut RawLuaBase) {}
    fn top(_: *mut RawLuaBase) -> c_int { 7 }
    fn int(_: *mut RawLuaBase, _: c_int) {}
    fn int_result(_: *mut RawLuaBase, _: c_int) -> c_int { 0 }
    fn pcall(_: *mut RawLuaBase, _: c_int, _: c_int, _: c_int) -> c_int { 0 }
    fn new_userdata(_: *mut RawLuaBase, _: c_uint) -> *mut c_void {
        core::ptr::null_mut()
    }
    fn get_userdata(_: *mut RawLuaBase, _: c_int) -> *mut c_void {
        core::ptr::null_mut()
    }
    fn create_meta_table(_: *mut RawLuaBase, _: *const c_char) -> c_int { 1 }
    fn bool_int(_: *mut RawLuaBase, _: c_int) -> bool { true }
    fn set_user_type(_: *mut RawLuaBase, _: c_int, _: *mut c_void) {}
    fn get_string(_: *mut RawLuaBase, _: c_int, _: *mut c_uint) -> *const c_char {
        core::ptr::null()
    }
    fn get_number(_: *mut RawLuaBase, _: c_int) -> c_double { 0.0 }
    fn get_bool(_: *mut RawLuaBase, _: c_int) -> bool { false }
    fn push_string(_: *mut RawLuaBase, _: *const c_char, _: c_uint) {}
    fn push_number(_: *mut RawLuaBase, _: c_double) {}
    fn push_bool(_: *mut RawLuaBase, value: bool) {
        PUSHED_BOOL.store(value, Ordering::Relaxed);
    }
    fn push_c_closure(_: *mut RawLuaBase, _: LuaCFunction, _: c_int) {}
    fn push_special(_: *mut RawLuaBase, _: SpecialIndex) {}
    fn set_state(_: *mut RawLuaBase, _: *mut RawLuaState) {}
}

fn test_vtable() -> RawLuaBaseVTable {
    RawLuaBaseVTable {
        top,
        push: int,
        pop: int,
        get_table: unused,
        get_field: unused,
        set_field: unused,
        create_table: void,
        set_table: unused,
        set_meta_table: int,
        get_meta_table: unused,
        call: unused,
        pcall,
        equal: unused,
        raw_equal: unused,
        insert: int,
        remove: int,
        next: int_result,
        new_userdata,
        throw_error: unused,
        check_type: unused,
        arg_error: unused,
        raw_get: int,
        raw_set: int,
        get_string,
        get_number,
        get_bool,
        get_c_function: unused,
        get_userdata,
        push_nil: void,
        push_string,
        push_number,
        push_bool,
        push_c_function: unused,
        push_c_closure,
        push_userdata: unused,
        reference_create: top,
        reference_free: int,
        reference_push: int,
        push_special,
        is_type: unused,
        get_type: int_result,
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
        push_meta_table: bool_int,
        push_user_type: unused,
        set_user_type,
    }
}

#[test]
fn typed_methods_dispatch_through_pinned_slots() {
    // Given: a fake object with the exact ILuaBase vtable shape.
    let vtable = test_vtable();
    let mut lua_base = RawLuaBase {
        vtable: &vtable,
        state: core::ptr::null_mut(),
    };

    // When: typed raw methods dispatch through their named slots.
    // SAFETY: the local object and vtable are live and use the exact test
    // layout expected by these raw calls.
    let height = unsafe { RawLuaBase::top(&mut lua_base) };
    // SAFETY: same live local object and exact test vtable.
    unsafe { RawLuaBase::push_bool(&mut lua_base, true) };

    // Then: each intended function receives the call.
    assert_eq!(height, 7);
    assert!(PUSHED_BOOL.load(Ordering::Relaxed));
}

#[test]
fn vtable_contains_exact_number_of_header_slots() {
    // Given: ILuaBase methods through SetUserType in the pinned header.
    // When: Rust lays out the private vtable prefix.
    // Then: it contains exactly 55 pointer-sized slots.
    assert_eq!(
        core::mem::size_of::<RawLuaBaseVTable>(),
        55 * core::mem::size_of::<*const ()>()
    );
}

#[test]
fn nullable_c_function_uses_one_pointer_slot() {
    assert_eq!(
        core::mem::size_of::<Option<LuaCFunction>>(),
        core::mem::size_of::<*const ()>()
    );
}

#[test]
fn raw_lua_base_matches_pinned_class_layout() {
    assert_eq!(
        core::mem::size_of::<RawLuaBase>(),
        2 * core::mem::size_of::<*const ()>()
    );
    assert_eq!(
        core::mem::align_of::<RawLuaBase>(),
        core::mem::align_of::<*const ()>()
    );
    assert_eq!(
        core::mem::offset_of!(RawLuaBase, state),
        core::mem::size_of::<*const ()>()
    );
}
