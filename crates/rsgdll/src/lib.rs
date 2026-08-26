//! Public facade for building Garry's Mod binary Lua modules.

pub use rsgdll_macros::{function, module};

/// Checked Lua APIs.
pub use rsgdll_lua as lua;

/// Module lifecycle and registration APIs.
pub use rsgdll_module as module;

/// Main-thread runtime services.
pub use rsgdll_runtime as runtime;

/// Executor-neutral background completion adapters.
#[cfg(feature = "async")]
pub use rsgdll_async as async_runtime;

/// Common developer-facing imports.
pub mod prelude {
    pub use rsgdll_lua::{
        FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, LuaBytes, LuaError, LuaFunction,
        LuaResult, LuaTable, RegistryReference, Stack, StackFrame, UserDataType,
    };
    pub use rsgdll_module::{
        BoxError, IntoLuaReturn, LuaStackValues, ModuleBuilder, install_userdata_gc,
    };
    pub use rsgdll_runtime::{CompletionQueue, CompletionSender, MainThread, completion_queue};
}

/// Low-level escape hatches reserved for the `raw` feature.
#[cfg(feature = "raw")]
pub mod raw {}

/// Framework plumbing used by generated code.
#[doc(hidden)]
pub mod __private {
    pub use rsgdll_lua as lua;
    pub use rsgdll_module as module;
    pub use rsgdll_runtime as runtime;
}
