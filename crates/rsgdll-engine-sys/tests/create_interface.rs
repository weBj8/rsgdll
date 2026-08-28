use std::ffi::{c_char, c_int, c_void};

use rsgdll_engine_sys::{
    CREATE_INTERFACE_SYMBOL, CreateInterfaceFn, ENGINE_LIBRARY, IFACE_FAILED, IFACE_OK,
    RawEngineServer, RawEngineServerVTable, UnusedEngineMethod, VENGINE_SERVER_VERSION,
};

unsafe extern "C" fn factory(_name: *const c_char, return_code: *mut c_int) -> *mut c_void {
    if !return_code.is_null() {
        // SAFETY: the test caller supplies a valid pointer to one `c_int`.
        unsafe { return_code.write(IFACE_FAILED) };
    }
    std::ptr::null_mut()
}

#[test]
fn source_factory_abi_matches_pinned_header() {
    let _: CreateInterfaceFn = factory;
    assert_eq!(
        CREATE_INTERFACE_SYMBOL.to_bytes_with_nul(),
        b"CreateInterface\0"
    );
    assert_eq!(ENGINE_LIBRARY.to_bytes_with_nul(), b"engine.so\0");
    assert_eq!(IFACE_OK, 0);
    assert_eq!(IFACE_FAILED, 1);
    assert_eq!(
        VENGINE_SERVER_VERSION.to_bytes_with_nul(),
        b"VEngineServer021\0"
    );
}

unsafe extern "C" fn change_level(
    _this: *mut RawEngineServer,
    _level: *const c_char,
    _landmark: *const c_char,
) {
}

unsafe extern "C" fn is_map_valid(_this: *mut RawEngineServer, _name: *const c_char) -> c_int {
    1
}

unsafe extern "C" fn is_dedicated_server(_this: *mut RawEngineServer) -> bool {
    true
}

unsafe extern "C" fn unused_engine_method() {}

unsafe extern "C" fn server_command(_this: *mut RawEngineServer, _command: *const c_char) {}

#[test]
fn engine_server_vtable_prefix_matches_pinned_interface() {
    let vtable = RawEngineServerVTable {
        change_level,
        is_map_valid,
        is_dedicated_server,
        before_server_command: [unused_engine_method as UnusedEngineMethod; 33],
        server_command,
    };
    assert!(std::ptr::fn_addr_eq(
        vtable.is_dedicated_server,
        is_dedicated_server as unsafe extern "C" fn(*mut RawEngineServer) -> bool
    ));
    assert!(std::ptr::fn_addr_eq(
        vtable.server_command,
        server_command as unsafe extern "C" fn(*mut RawEngineServer, *const c_char)
    ));
}
