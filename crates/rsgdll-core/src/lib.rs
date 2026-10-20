//! Safe layer of rsgdll: `Lua` handle, protected ops, multi-state runtime, refs, errors,
//! trampolines and value conversion.

#[cfg(panic = "abort")]
compile_error!("rsgdll requires `panic = \"unwind\"` (catch_unwind in trampolines)");

pub mod error;
pub mod lua;
pub mod protected;
pub mod refs;
pub mod state;
pub mod trampoline;

pub use error::{ConvError, Context, Error, ErrorKind, Result, set_backtrace};
pub use lua::{Lua, StackGuard};
pub use refs::LuaRef;
