//! Checked, main-thread-bound Lua abstractions.

#![deny(unsafe_op_in_unsafe_fn)]

mod convert;
mod error;
mod lua;
mod stack;

pub use convert::{FromLua, IntoLua};
pub use error::{LuaError, LuaResult};
pub use lua::Lua;
pub use rsgdll_platform::__private::{LuaCFunction, LuaType};
pub use stack::{Stack, StackFrame};
