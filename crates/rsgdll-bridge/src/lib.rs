//! Minimal C++ Lua error firewall.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_int, c_void};

use rsgdll_platform::__private::{
    LuaCFunction, RSGDLL_ABI_CREATE_META_TABLE_SLOT, RSGDLL_ABI_CREATE_TABLE_SLOT,
    RSGDLL_ABI_GET_TYPE_SLOT, RSGDLL_ABI_LUA_BASE_OFFSET, RSGDLL_ABI_NEW_USERDATA_SLOT,
    RSGDLL_ABI_NEXT_SLOT, RSGDLL_ABI_PCALL_SLOT, RSGDLL_ABI_POP_SLOT, RSGDLL_ABI_PUSH_BOOL_SLOT,
    RSGDLL_ABI_PUSH_CLOSURE_SLOT, RSGDLL_ABI_PUSH_META_TABLE_SLOT, RSGDLL_ABI_PUSH_NIL_SLOT,
    RSGDLL_ABI_PUSH_NUMBER_SLOT, RSGDLL_ABI_PUSH_SLOT, RSGDLL_ABI_PUSH_SPECIAL_SLOT,
    RSGDLL_ABI_PUSH_STRING_SLOT, RSGDLL_ABI_RAW_GET_SLOT, RSGDLL_ABI_RAW_SET_SLOT,
    RSGDLL_ABI_REFERENCE_CREATE_SLOT, RSGDLL_ABI_REFERENCE_FREE_SLOT,
    RSGDLL_ABI_REFERENCE_PUSH_SLOT, RSGDLL_ABI_REMOVE_SLOT, RSGDLL_ABI_SET_META_TABLE_SLOT,
    RSGDLL_ABI_SET_STATE_SLOT, RSGDLL_ABI_SET_USER_TYPE_SLOT, RSGDLL_ABI_THROW_ERROR_SLOT,
    RSGDLL_ABI_TOP_SLOT, RawLuaState,
};
#[cfg(all(feature = "debug", feature = "test-support"))]
use rsgdll_platform::__private::{
    LuaGetHook, LuaGetHookCount, LuaGetHookMask, LuaGetInfo, LuaGetLocal, LuaGetStack,
    LuaGetUpvalue, LuaSetHook, LuaSetLocal, LuaSetUpvalue,
};
#[cfg(feature = "debug")]
use rsgdll_platform::__private::{LuaHook, RawLuaDebug};

#[cfg(all(feature = "debug", not(feature = "test-support")))]
mod debug_native;

/// Capacity of the stack-owned error buffer supplied by the C++ trampoline.
pub const ERROR_BUFFER_CAPACITY: u32 = 32 * 1024;
/// Maximum number of Lua return values staged outside the Lua stack.
pub const RETURN_SLOT_CAPACITY: usize = 16;
/// Capacity of copied string return data.
pub const RETURN_BYTE_CAPACITY: usize = 4096;

pub const RETURN_NIL: u32 = 0;
pub const RETURN_BOOL: u32 = 1;
pub const RETURN_NUMBER: u32 = 2;
pub const RETURN_STRING: u32 = 3;
pub const RETURN_MODE_STAGED: u32 = 0;
pub const RETURN_MODE_STACK: u32 = 1;

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_RUST_ERROR: i32 = 1;
pub const STATUS_RUST_PANIC: i32 = 2;
pub const STATUS_INTERNAL_ERROR: i32 = 3;

pub const OP_PUSH: u32 = 1;
pub const OP_POP: u32 = 2;
pub const OP_CREATE_TABLE: u32 = 3;
pub const OP_PCALL: u32 = 4;
pub const OP_SET_META_TABLE: u32 = 5;
pub const OP_NEW_USERDATA: u32 = 6;
pub const OP_RAW_GET: u32 = 7;
pub const OP_RAW_SET: u32 = 8;
pub const OP_NEXT: u32 = 9;
pub const OP_PUSH_NIL: u32 = 10;
pub const OP_PUSH_STRING: u32 = 11;
pub const OP_PUSH_NUMBER: u32 = 12;
pub const OP_PUSH_BOOL: u32 = 13;
pub const OP_PUSH_C_CLOSURE: u32 = 14;
pub const OP_REFERENCE_CREATE: u32 = 15;
pub const OP_REFERENCE_FREE: u32 = 16;
pub const OP_REFERENCE_PUSH: u32 = 17;
pub const OP_PUSH_SPECIAL: u32 = 18;
pub const OP_CREATE_META_TABLE: u32 = 19;
pub const OP_PUSH_META_TABLE: u32 = 20;
pub const OP_SET_USER_TYPE: u32 = 21;

/// One function registration shared with the C++ module-opening firewall.
#[doc(hidden)]
#[repr(C)]
pub struct ModuleRegistration {
    pub name: *const u8,
    pub name_length: u32,
    pub callback_id: u32,
}

/// Module-local ABI layout passed to C++ without exporting native data symbols.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AbiLayout {
    pub lua_base_offset: usize,
    pub top_slot: usize,
    pub push_slot: usize,
    pub pop_slot: usize,
    pub create_table_slot: usize,
    pub set_meta_table_slot: usize,
    pub pcall_slot: usize,
    pub remove_slot: usize,
    pub next_slot: usize,
    pub new_userdata_slot: usize,
    pub throw_error_slot: usize,
    pub raw_get_slot: usize,
    pub raw_set_slot: usize,
    pub push_nil_slot: usize,
    pub push_string_slot: usize,
    pub push_number_slot: usize,
    pub push_bool_slot: usize,
    pub push_closure_slot: usize,
    pub reference_create_slot: usize,
    pub reference_free_slot: usize,
    pub reference_push_slot: usize,
    pub push_special_slot: usize,
    pub get_type_slot: usize,
    pub set_state_slot: usize,
    pub create_meta_table_slot: usize,
    pub push_meta_table_slot: usize,
    pub set_user_type_slot: usize,
}

/// ABI values compiled into this module and shared with its private C++ bridge.
#[doc(hidden)]
pub static ABI_LAYOUT: AbiLayout = AbiLayout {
    lua_base_offset: RSGDLL_ABI_LUA_BASE_OFFSET,
    top_slot: RSGDLL_ABI_TOP_SLOT,
    push_slot: RSGDLL_ABI_PUSH_SLOT,
    pop_slot: RSGDLL_ABI_POP_SLOT,
    create_table_slot: RSGDLL_ABI_CREATE_TABLE_SLOT,
    set_meta_table_slot: RSGDLL_ABI_SET_META_TABLE_SLOT,
    pcall_slot: RSGDLL_ABI_PCALL_SLOT,
    remove_slot: RSGDLL_ABI_REMOVE_SLOT,
    next_slot: RSGDLL_ABI_NEXT_SLOT,
    new_userdata_slot: RSGDLL_ABI_NEW_USERDATA_SLOT,
    throw_error_slot: RSGDLL_ABI_THROW_ERROR_SLOT,
    raw_get_slot: RSGDLL_ABI_RAW_GET_SLOT,
    raw_set_slot: RSGDLL_ABI_RAW_SET_SLOT,
    push_nil_slot: RSGDLL_ABI_PUSH_NIL_SLOT,
    push_string_slot: RSGDLL_ABI_PUSH_STRING_SLOT,
    push_number_slot: RSGDLL_ABI_PUSH_NUMBER_SLOT,
    push_bool_slot: RSGDLL_ABI_PUSH_BOOL_SLOT,
    push_closure_slot: RSGDLL_ABI_PUSH_CLOSURE_SLOT,
    reference_create_slot: RSGDLL_ABI_REFERENCE_CREATE_SLOT,
    reference_free_slot: RSGDLL_ABI_REFERENCE_FREE_SLOT,
    reference_push_slot: RSGDLL_ABI_REFERENCE_PUSH_SLOT,
    push_special_slot: RSGDLL_ABI_PUSH_SPECIAL_SLOT,
    get_type_slot: RSGDLL_ABI_GET_TYPE_SLOT,
    set_state_slot: RSGDLL_ABI_SET_STATE_SLOT,
    create_meta_table_slot: RSGDLL_ABI_CREATE_META_TABLE_SLOT,
    push_meta_table_slot: RSGDLL_ABI_PUSH_META_TABLE_SLOT,
    set_user_type_slot: RSGDLL_ABI_SET_USER_TYPE_SLOT,
};

/// One POD Lua operation executed by C++ inside `lua_cpcall`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LuaOperation {
    pub opcode: u32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub pointer: *const c_void,
    pub length: u32,
    pub reserved: u32,
    pub number: f64,
    pub result_pointer: *mut c_void,
    pub result_integer: i64,
}

impl LuaOperation {
    #[must_use]
    pub const fn new(opcode: u32) -> Self {
        Self {
            opcode,
            a: 0,
            b: 0,
            c: 0,
            pointer: std::ptr::null(),
            length: 0,
            reserved: 0,
            number: 0.0,
            result_pointer: std::ptr::null_mut(),
            result_integer: 0,
        }
    }
}

/// One POD Lua return value written by Rust and consumed by C++.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ReturnSlot {
    pub tag: u32,
    pub offset: u32,
    pub length: u32,
    pub reserved: u32,
    pub number: f64,
}

/// C++-owned staging storage filled before Rust returns.
#[repr(C)]
pub struct ReturnBuffer {
    pub slots: [ReturnSlot; RETURN_SLOT_CAPACITY],
    pub bytes: [u8; RETURN_BYTE_CAPACITY],
}

/// POD result returned after all Rust dispatcher frames have unwound normally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct DispatchResult {
    pub status: i32,
    pub return_count: i32,
    pub error_length: u32,
    pub return_mode: u32,
}

impl DispatchResult {
    #[must_use]
    pub const fn success(return_count: i32) -> Self {
        Self {
            status: STATUS_SUCCESS,
            return_count,
            error_length: 0,
            return_mode: RETURN_MODE_STAGED,
        }
    }

    #[must_use]
    pub const fn stack_success(return_count: i32) -> Self {
        Self {
            status: STATUS_SUCCESS,
            return_count,
            error_length: 0,
            return_mode: RETURN_MODE_STACK,
        }
    }

    #[must_use]
    pub const fn failure(status: i32, error_length: u32) -> Self {
        Self {
            status,
            return_count: 0,
            error_length,
            return_mode: RETURN_MODE_STAGED,
        }
    }
}

/// Rust dispatcher signature registered with the generic C++ trampoline.
pub type Dispatcher =
    unsafe extern "C" fn(*mut RawLuaState, *mut c_char, u32, *mut ReturnBuffer) -> DispatchResult;

#[cfg(feature = "debug")]
pub type DebugDispatcher = unsafe extern "C" fn(*mut RawLuaState, *mut RawLuaDebug);

unsafe extern "C" {
    #[cfg(feature = "test-support")]
    fn rsgdll_bridge_enable_test_mode(layout: *const AbiLayout);
    #[cfg(feature = "test-support")]
    fn rsgdll_bridge_test_last_dispatch_status() -> c_int;
    fn rsgdll_bridge_execute(
        state: *mut RawLuaState,
        lua_base: *mut c_void,
        operation: *mut LuaOperation,
    ) -> c_int;
    fn rsgdll_bridge_set_dispatcher(dispatcher: Dispatcher);
    fn rsgdll_bridge_trampoline(state: *mut RawLuaState) -> c_int;
    #[cfg(all(feature = "debug", not(feature = "test-support")))]
    fn rsgdll_bridge_debug_set_dispatcher(dispatcher: DebugDispatcher);
    #[cfg(all(feature = "debug", not(feature = "test-support")))]
    fn rsgdll_bridge_debug_hook(state: *mut RawLuaState, record: *mut RawLuaDebug);
}

#[cfg(all(feature = "debug", feature = "test-support"))]
mod debug_test_support {
    use std::sync::{Mutex, OnceLock};

    use super::{
        DebugDispatcher, LuaGetHook, LuaGetHookCount, LuaGetHookMask, LuaGetInfo, LuaGetLocal,
        LuaGetStack, LuaGetUpvalue, LuaHook, LuaSetHook, LuaSetLocal, LuaSetUpvalue, RawLuaDebug,
        RawLuaState,
    };

    #[derive(Clone, Copy)]
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

    static API: OnceLock<DebugApi> = OnceLock::new();
    static DISPATCHER: Mutex<Option<DebugDispatcher>> = Mutex::new(None);

    pub fn install(api: DebugApi) {
        let _ = API.set(api);
    }

    pub fn api() -> &'static DebugApi {
        API.get().expect("debug test API is not installed")
    }

    pub fn set_dispatcher(dispatcher: DebugDispatcher) {
        *DISPATCHER.lock().expect("debug dispatcher lock") = Some(dispatcher);
    }

    pub unsafe extern "C" fn hook(state: *mut RawLuaState, record: *mut RawLuaDebug) {
        let dispatcher = *DISPATCHER.lock().expect("debug dispatcher lock");
        if let Some(dispatcher) = dispatcher {
            // SAFETY: test fixture invokes this with its live state and record.
            unsafe { dispatcher(state, record) };
        }
    }

    pub fn hook_pointer() -> LuaHook {
        Some(hook)
    }
}

#[doc(hidden)]
#[cfg(all(feature = "debug", feature = "test-support"))]
pub use debug_test_support::DebugApi;

#[doc(hidden)]
#[cfg(feature = "test-support")]
pub mod __private {
    /// Enables the fake-`ILuaBase` execution path used by workspace tests.
    ///
    /// ```compile_fail
    /// rsgdll_bridge::__private::enable_test_mode();
    /// ```
    ///
    /// # Safety
    ///
    /// Every setup vtable method used before the protected call must return
    /// normally rather than raising a Lua error or performing `longjmp`.
    pub unsafe fn enable_test_mode() {
        // SAFETY: the static layout remains valid for the process lifetime.
        unsafe { super::rsgdll_bridge_enable_test_mode(&super::ABI_LAYOUT) };
    }

    /// Returns the dispatcher status observed by this thread's last trampoline call.
    #[must_use]
    pub fn last_dispatch_status() -> i32 {
        // SAFETY: the C++ function reads only thread-local test instrumentation.
        unsafe { super::rsgdll_bridge_test_last_dispatch_status() }
    }
}

/// Executes one potentially throwing Lua operation under Lua's native
/// protected-call boundary.
///
/// # Safety
///
/// Pointers must refer to the same live Lua state and `ILuaBase`. Any pointer
/// stored in `operation` must satisfy the selected opcode's ABI contract.
pub unsafe fn execute(
    state: *mut RawLuaState,
    lua_base: *mut c_void,
    operation: &mut LuaOperation,
) -> c_int {
    // SAFETY: caller upholds the shared-state and opcode-specific contracts.
    unsafe { rsgdll_bridge_execute(state, lua_base, operation) }
}

/// Replaces the process-wide dispatcher used by the C++ trampoline.
///
/// # Safety
///
/// The dispatcher must treat all pointers as borrowed for one call, must not
/// unwind, and must report only return values it actually leaves on the stack
/// or stages in the supplied buffer.
pub unsafe fn set_dispatcher(dispatcher: Dispatcher) {
    // SAFETY: function pointer has the exact C ABI expected by the bridge and
    // remains valid for the process lifetime.
    unsafe { rsgdll_bridge_set_dispatcher(dispatcher) };
}

/// Returns the one generic Lua callback implemented by the C++ firewall.
#[must_use]
pub fn trampoline() -> LuaCFunction {
    rsgdll_bridge_trampoline
}

#[cfg(feature = "debug")]
/// Calls `lua_getstack` through the pinned bridge.
///
/// # Safety
///
/// Pointers must belong to one live main-thread Lua callback.
pub unsafe fn debug_get_stack(
    state: *mut RawLuaState,
    level: c_int,
    record: *mut RawLuaDebug,
) -> c_int {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().get_stack)(state, level, record) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(0, |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.get_stack)(state, level, record) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_getinfo` through the pinned bridge.
///
/// # Safety
///
/// `what` must be NUL-terminated; state and record must be live and related.
pub unsafe fn debug_get_info(
    state: *mut RawLuaState,
    what: *const c_char,
    record: *mut RawLuaDebug,
) -> c_int {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().get_info)(state, what, record) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(0, |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.get_info)(state, what, record) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_getlocal` through the pinned bridge.
///
/// # Safety
///
/// State and record must identify one live Lua frame.
pub unsafe fn debug_get_local(
    state: *mut RawLuaState,
    record: *const RawLuaDebug,
    index: c_int,
) -> *const c_char {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().get_local)(state, record, index) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(std::ptr::null(), |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.get_local)(state, record, index) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_setlocal` through the pinned bridge.
///
/// # Safety
///
/// State and record must identify one live frame with a value at stack top.
pub unsafe fn debug_set_local(
    state: *mut RawLuaState,
    record: *const RawLuaDebug,
    index: c_int,
) -> *const c_char {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().set_local)(state, record, index) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(std::ptr::null(), |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.set_local)(state, record, index) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_getupvalue` through the pinned bridge.
///
/// # Safety
///
/// State must be live and `function_index` must identify a Lua function.
pub unsafe fn debug_get_upvalue(
    state: *mut RawLuaState,
    function_index: c_int,
    index: c_int,
) -> *const c_char {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().get_upvalue)(state, function_index, index) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(std::ptr::null(), |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.get_upvalue)(state, function_index, index) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_setupvalue` through the pinned bridge.
///
/// # Safety
///
/// State must be live, the function index valid, and a value at stack top.
pub unsafe fn debug_set_upvalue(
    state: *mut RawLuaState,
    function_index: c_int,
    index: c_int,
) -> *const c_char {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua debug API contract.
        unsafe { (debug_test_support::api().set_upvalue)(state, function_index, index) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(std::ptr::null(), |api| {
            // SAFETY: caller upholds the Lua debug API contract.
            unsafe { (api.set_upvalue)(state, function_index, index) }
        })
    }
}

#[cfg(feature = "debug")]
/// Calls `lua_sethook` through the pinned bridge.
///
/// # Safety
///
/// State must be live and the hook must obey the pinned Lua hook ABI.
pub unsafe fn debug_set_hook(
    state: *mut RawLuaState,
    hook: LuaHook,
    mask: c_int,
    count: c_int,
) -> c_int {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller upholds the Lua hook contract.
        unsafe { (debug_test_support::api().set_hook)(state, hook, mask, count) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(0, |api| {
            // SAFETY: caller upholds the Lua hook contract.
            unsafe { (api.set_hook)(state, hook, mask, count) }
        })
    }
}

#[cfg(feature = "debug")]
/// Reads the current hook from one live Lua state.
///
/// # Safety
///
/// State must be live and main-thread-owned.
pub unsafe fn debug_get_hook(state: *mut RawLuaState) -> LuaHook {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller supplies a live Lua state.
        unsafe { (debug_test_support::api().get_hook)(state) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().and_then(|api| {
            // SAFETY: caller supplies a live Lua state.
            unsafe { (api.get_hook)(state) }
        })
    }
}

#[cfg(feature = "debug")]
/// Reads the current hook mask from one live Lua state.
///
/// # Safety
///
/// State must be live and main-thread-owned.
pub unsafe fn debug_get_hook_mask(state: *mut RawLuaState) -> c_int {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller supplies a live Lua state.
        unsafe { (debug_test_support::api().get_hook_mask)(state) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(0, |api| {
            // SAFETY: caller supplies a live Lua state.
            unsafe { (api.get_hook_mask)(state) }
        })
    }
}

#[cfg(feature = "debug")]
/// Reads the current hook count from one live Lua state.
///
/// # Safety
///
/// State must be live and main-thread-owned.
pub unsafe fn debug_get_hook_count(state: *mut RawLuaState) -> c_int {
    #[cfg(feature = "test-support")]
    {
        // SAFETY: caller supplies a live Lua state.
        unsafe { (debug_test_support::api().get_hook_count)(state) }
    }
    #[cfg(not(feature = "test-support"))]
    {
        debug_native::api().map_or(0, |api| {
            // SAFETY: caller supplies a live Lua state.
            unsafe { (api.get_hook_count)(state) }
        })
    }
}

#[cfg(feature = "debug")]
/// Registers the permanent Rust debug dispatcher.
///
/// # Safety
///
/// Dispatcher must not unwind and may borrow pointers only for one call.
pub unsafe fn set_debug_dispatcher(dispatcher: DebugDispatcher) {
    #[cfg(feature = "test-support")]
    debug_test_support::set_dispatcher(dispatcher);
    #[cfg(not(feature = "test-support"))]
    {
        // SAFETY: function pointer has the exact permanent bridge ABI.
        unsafe { rsgdll_bridge_debug_set_dispatcher(dispatcher) };
    }
}

#[cfg(feature = "debug")]
#[must_use]
pub fn debug_hook() -> LuaHook {
    #[cfg(feature = "test-support")]
    {
        debug_test_support::hook_pointer()
    }
    #[cfg(not(feature = "test-support"))]
    {
        Some(rsgdll_bridge_debug_hook)
    }
}

#[doc(hidden)]
#[cfg(all(feature = "debug", feature = "test-support"))]
pub fn install_debug_test_api(api: DebugApi) {
    debug_test_support::install(api);
}

const _: () = assert!(STATUS_SUCCESS != STATUS_RUST_ERROR);
const _: () = assert!(STATUS_RUST_ERROR != STATUS_RUST_PANIC);
const _: () = assert!(STATUS_RUST_PANIC != STATUS_INTERNAL_ERROR);
const _: () = assert!(std::mem::size_of::<DispatchResult>() == 16);
const _: () = assert!(std::mem::size_of::<ReturnSlot>() == 24);
const _: () = assert!(std::mem::size_of::<ReturnBuffer>() == 4480);
const _: () = assert!(
    std::mem::size_of::<LuaOperation>()
        == if cfg!(all(target_os = "linux", target_pointer_width = "32")) {
            48
        } else {
            56
        }
);
const _: () =
    assert!(std::mem::size_of::<ModuleRegistration>() == std::mem::size_of::<*const u8>() + 8);
const _: () = assert!(std::mem::size_of::<AbiLayout>() == 27 * std::mem::size_of::<usize>());

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::ffi::c_int;

    use super::{DispatchResult, ERROR_BUFFER_CAPACITY};

    unsafe extern "C" {
        fn rsgdll_bridge_test_accepts_dispatch_result(
            result: DispatchResult,
            entry_top: c_int,
            exit_top: c_int,
        ) -> bool;
    }

    #[test]
    fn dispatcher_result_rejects_unbacked_stack_values() {
        // Given: a dispatcher claims one stack return without pushing it.
        let result = DispatchResult::stack_success(1);

        // When: C++ validates the result against unchanged stack height.
        // Then: it rejects the unbacked return count.
        assert!(!unsafe { rsgdll_bridge_test_accepts_dispatch_result(result, 0, 0) });
    }

    #[test]
    fn diagnostic_buffer_preserves_the_architecture_backtrace_budget() {
        assert_eq!(ERROR_BUFFER_CAPACITY, 32 * 1024);
    }

    #[test]
    fn vtable_pointer_is_loaded_bytewise_when_calling_lua() {
        // Given: the C++ source implementing Lua vtable dispatch.
        let source = include_str!("firewall.cpp");

        // When: the vtable load expression is inspected.
        // Then: it copies object representation instead of aliasing as `void ***`.
        assert!(source.contains("std::memcpy(&vtable, lua_base, sizeof(vtable));"));
        assert!(!source.contains("reinterpret_cast<void ***"));
    }
}
