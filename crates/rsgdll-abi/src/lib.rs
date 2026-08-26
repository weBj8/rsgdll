//! Raw Garry's Mod and Lua ABI definitions.
//!
//! This crate is internal, dependency-free, and intentionally exposes only raw
//! pointers and explicitly unsafe ABI calls. See `docs/abi-reference.md` for
//! the pinned upstream definitions.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "rsgdll-abi has only a header-defined layout for Linux x86_64; \
     this target has no reviewed ABI description"
);

mod lua_base;
mod state;
mod types;

pub use lua_base::RawLuaBase;
pub use state::{RAW_LUA_BASE_OFFSET, RawLuaState};
pub use types::{LuaCFunction, LuaType, RawUserData, SpecialIndex};
