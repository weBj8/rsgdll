//! Checked, main-thread-bound Lua abstractions.

#![deny(unsafe_op_in_unsafe_fn)]

mod convert;
mod error;
mod function;
mod lua;
mod multi;
mod reference;
#[cfg(feature = "serde")]
pub mod serde;
mod stack;
mod string;
mod table;
mod userdata;

pub use convert::{FromLua, IntoLua};
pub use error::{LuaError, LuaResult};
pub use function::LuaFunction;
pub use lua::Lua;
pub use multi::{FromLuaMulti, IntoLuaMulti};
pub use reference::RegistryReference;
pub use rsgdll_platform::__private::{LuaCFunction, LuaType};
pub use stack::{Stack, StackFrame};
pub use string::LuaBytes;
pub use table::LuaTable;
pub use userdata::UserDataType;
