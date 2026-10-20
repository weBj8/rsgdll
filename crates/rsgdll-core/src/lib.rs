//! Safe layer of rsgdll: `Lua` handle, protected ops, multi-state runtime, refs, errors,
//! trampolines and value conversion.

#[cfg(panic = "abort")]
compile_error!("rsgdll requires `panic = \"unwind\"` (catch_unwind in trampolines)");

pub mod chunk;
pub mod conv;
pub mod error;
pub mod export;
pub mod interface;
pub mod lua;
pub mod module;
pub mod protected;
pub mod realm;
pub mod refs;
pub mod rt;
pub mod state;
pub mod tracked;
pub mod trampoline;

pub use conv::*;
pub use error::{Context, ConvError, Error, ErrorKind, Result, set_backtrace};
pub use linkme;
pub use lua::{Lua, StackGuard};
pub use realm::Realm;
pub use refs::LuaRef;
