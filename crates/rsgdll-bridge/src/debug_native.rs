use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

use rsgdll_platform::__private::{
    LuaGetHook, LuaGetHookCount, LuaGetHookMask, LuaGetInfo, LuaGetLocal, LuaGetStack,
    LuaGetUpvalue, LuaSetHook, LuaSetLocal, LuaSetUpvalue,
};

pub struct DebugApi {
    pub get_stack: LuaGetStack,
    pub get_info: LuaGetInfo,
    pub get_local: LuaGetLocal,
    pub set_local: LuaSetLocal,
    pub get_upvalue: LuaGetUpvalue,
    pub set_upvalue: LuaSetUpvalue,
    pub set_hook: LuaSetHook,
    pub get_hook: LuaGetHook,
    pub get_hook_mask: LuaGetHookMask,
    pub get_hook_count: LuaGetHookCount,
}

static API: OnceLock<Option<DebugApi>> = OnceLock::new();

pub fn api() -> Option<&'static DebugApi> {
    API.get_or_init(load).as_ref()
}

fn load() -> Option<DebugApi> {
    // SAFETY: the returned module is Garry's Mod's already loaded Lua runtime;
    // each symbol is converted to its exact pinned C ABI declaration.
    unsafe {
        let library = open_lua_runtime();
        if library.is_null() {
            return None;
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let pointer = load_symbol(library, concat!($name, "\0").as_ptr().cast());
                if pointer.is_null() {
                    return None;
                }
                std::mem::transmute::<*mut c_void, $ty>(pointer)
            }};
        }
        Some(DebugApi {
            get_stack: symbol!("lua_getstack", LuaGetStack),
            get_info: symbol!("lua_getinfo", LuaGetInfo),
            get_local: symbol!("lua_getlocal", LuaGetLocal),
            set_local: symbol!("lua_setlocal", LuaSetLocal),
            get_upvalue: symbol!("lua_getupvalue", LuaGetUpvalue),
            set_upvalue: symbol!("lua_setupvalue", LuaSetUpvalue),
            set_hook: symbol!("lua_sethook", LuaSetHook),
            get_hook: symbol!("lua_gethook", LuaGetHook),
            get_hook_mask: symbol!("lua_gethookmask", LuaGetHookMask),
            get_hook_count: symbol!("lua_gethookcount", LuaGetHookCount),
        })
    }
}

#[cfg(target_os = "linux")]
unsafe fn open_lua_runtime() -> *mut c_void {
    const RTLD_NOW: i32 = 2;
    for name in [
        c"lua_shared.so",
        c"lua_shared_srv.so",
        c"./garrysmod/bin/linux64/lua_shared.so",
        c"./garrysmod/bin/lua_shared_srv.so",
    ] {
        // SAFETY: names are NUL-terminated and handles intentionally remain
        // open for the module's process lifetime.
        let library = unsafe { dlopen(name.as_ptr(), RTLD_NOW) };
        if !library.is_null() {
            return library;
        }
    }
    std::ptr::null_mut()
}

#[cfg(target_os = "linux")]
unsafe fn load_symbol(library: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: library is a live dlopen handle and name is NUL-terminated.
    unsafe { dlsym(library, name) }
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(library: *mut c_void, name: *const c_char) -> *mut c_void;
}

#[cfg(target_os = "windows")]
unsafe fn open_lua_runtime() -> *mut c_void {
    for name in [c"lua_shared.dll", c"lua_shared_srv.dll"] {
        // SAFETY: names are NUL-terminated and GetModuleHandle does not alter
        // the loaded module's lifetime.
        let library = unsafe { get_module_handle(name.as_ptr()) };
        if !library.is_null() {
            return library;
        }
    }
    std::ptr::null_mut()
}

#[cfg(target_os = "windows")]
unsafe fn load_symbol(library: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: library is a live module handle and name is NUL-terminated.
    unsafe { get_proc_address(library, name) }
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetModuleHandleA"]
    fn get_module_handle(name: *const c_char) -> *mut c_void;
    #[link_name = "GetProcAddress"]
    fn get_proc_address(library: *mut c_void, name: *const c_char) -> *mut c_void;
}

const _: () = assert!(std::mem::size_of::<LuaGetStack>() == std::mem::size_of::<*mut c_void>());
