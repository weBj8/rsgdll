//! Public facade for building Garry's Mod binary Lua modules.

pub use rsgdll_macros::{function, module};

/// Checked Lua APIs.
///
/// Raw callback types and state construction are not part of this namespace.
///
/// ```compile_fail
/// use rsgdll::lua::LuaCFunction;
/// ```
///
/// ```compile_fail
/// let state = std::ptr::null_mut();
/// let _ = unsafe { rsgdll::lua::Lua::from_raw(state) };
/// ```
///
/// Bridge operation codes and statuses are not public API.
///
/// ```compile_fail
/// let _ = rsgdll::lua::LuaError::ProtectedOperation { opcode: 1, status: -1 };
/// ```
///
/// Debug frames cannot escape their hook callback.
///
/// ```compile_fail
/// use rsgdll::lua::{DebugContext, DebugFrame};
///
/// fn leak(context: &mut DebugContext<'_>) -> DebugFrame<'static, 'static> {
///     context.current_frame()
/// }
/// ```
pub mod lua {
    pub use rsgdll_lua::{
        FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, LuaBytes, LuaError, LuaFunction,
        LuaResult, LuaTable, LuaType, RegistryReference, Stack, StackFrame, UserDataType,
    };

    #[cfg(feature = "debug")]
    pub use rsgdll_lua::{
        DebugContext, DebugEvent, DebugFrame, DebugFrameInfo, DebugHook, DebugHookGuard,
        DebugLocal, DebugMask, DebugUpvalue,
    };

    #[cfg(feature = "serde")]
    pub use rsgdll_lua::serde;
}

/// Module lifecycle and registration APIs.
///
/// Dispatcher and registration plumbing is available only to generated code.
///
/// ```compile_fail
/// use rsgdll::module::{RawRegistration, install_dispatcher, trampoline};
/// ```
pub mod module {
    pub use rsgdll_module::{
        BoxError, Function, IntoLuaReturn, LuaStackValues, ModuleBuilder, ReturnError, ReturnWriter,
    };
}

/// Main-thread runtime services.
///
/// Main-thread capability construction remains framework-internal.
///
/// ```compile_fail
/// let _ = unsafe { rsgdll::runtime::MainThread::__from_callback() };
/// ```
pub mod runtime {
    pub use rsgdll_runtime::{CompletionQueue, CompletionSender, MainThread, completion_queue};
}

/// Executor-neutral background completion adapters.
#[cfg(feature = "async")]
pub use rsgdll_async as async_runtime;

/// Checked Source engine interfaces.
#[cfg(feature = "engine")]
pub use rsgdll_engine as engine;

/// Explicit signature scanning APIs.
#[cfg(feature = "sigscan")]
pub use rsgdll_sigscan as sigscan;

/// Explicitly unsafe detouring primitives.
#[cfg(feature = "detour")]
pub use rsgdll_detour as detour;

/// Common developer-facing imports.
pub mod prelude {
    #[cfg(feature = "debug")]
    pub use rsgdll_lua::{
        DebugContext, DebugEvent, DebugFrame, DebugFrameInfo, DebugHook, DebugHookGuard,
        DebugLocal, DebugMask, DebugUpvalue,
    };
    pub use rsgdll_lua::{
        FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, LuaBytes, LuaError, LuaFunction,
        LuaResult, LuaTable, RegistryReference, Stack, StackFrame, UserDataType,
    };
    pub use rsgdll_module::{
        BoxError, Function, IntoLuaReturn, LuaStackValues, ModuleBuilder, ReturnError, ReturnWriter,
    };
    pub use rsgdll_runtime::{CompletionQueue, CompletionSender, MainThread, completion_queue};
}

/// Reserved low-level namespace.
///
/// Version 0.1 intentionally exposes no raw ABI items. Future escape hatches
/// may be added here without making internal crates normal dependencies.
#[cfg(feature = "raw")]
pub mod raw {}

/// Framework plumbing used by generated code.
///
/// ```compile_fail
/// use rsgdll::__private::lua::LuaCFunction;
/// ```
///
/// ```compile_fail
/// use rsgdll::lua::StackFrame;
///
/// fn mint(frame: &StackFrame<'_, '_>) {
///     let _ = rsgdll::__private::runtime::main_thread_from_callback(frame);
/// }
/// ```
#[doc(hidden)]
pub mod __private {
    pub mod lua {
        pub use rsgdll_lua::StackFrame;
    }

    pub mod module {
        pub use rsgdll_module::{
            AbiLayout, BoxError, Function, RawRegistration, ReturnWriter, initialize_module,
        };
    }

    pub mod runtime {
        use rsgdll_lua::StackFrame;
        pub use rsgdll_runtime::MainThread;

        /// # Safety
        ///
        /// Caller must be generated callback glue holding `frame` inside the
        /// framework's GMod main-thread dispatcher.
        #[must_use]
        pub unsafe fn main_thread_from_callback(_frame: &StackFrame<'_, '_>) -> MainThread {
            // SAFETY: a checked callback frame exists only while the framework
            // dispatcher owns the GMod main thread.
            unsafe { rsgdll_runtime::__private::main_thread_from_callback() }
        }
    }
}
