//! Public facade for building Garry's Mod binary Lua modules.

/// Checked Lua APIs.
pub use rsgdll_lua as lua;

/// Module lifecycle and registration APIs.
pub use rsgdll_module as module;

/// Main-thread runtime services.
pub use rsgdll_runtime as runtime;

/// Common developer-facing imports.
pub mod prelude {
    pub use rsgdll_lua::{FromLua, IntoLua, Lua, LuaError, LuaResult, Stack, StackFrame};
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
