//! rsgdll: Garry's Mod binary modules in plain Rust. `use rsgdll::prelude::*;`

pub use rsgdll_core::{Context, ConvError, Error, ErrorKind, Lua, Result, anyhow, set_backtrace};
pub use rsgdll_core::conv::*;
pub use rsgdll_macros::{LuaUserData, LuaValue, function, interface, lua, methods, module, postclose, postopen, preclose, preopen};
pub use rsgdll_core::{Realm, interface::{LuaInterface, LuaShared, RealmInterface}};

pub use rsgdll_gmod::hook_marker;
pub use rsgdll_gmod::{concommand, cvars, hook, hooks, net, timer};
/// Attribute forms (`#[rsgdll::hook(Think)]`, `#[rsgdll::timer(every = "1s")]`); macros and
/// modules live in separate namespaces, so `rsgdll::hook::add` keeps working.
pub use rsgdll_macros::{hook, timer};
pub use rsgdll_core::rt::{LocalLua, LuaHandle, sleep, spawn_local, spawn_worker};
/// Callback param marker for async exports (`#[lua(callback)] cb: Callback`).
pub type Callback = LuaFunction;

#[cfg(feature = "tokio")]
pub mod tokio {
    pub use rsgdll_core::rt::tokio_rt::{runtime, spawn};
}

/// Engine interfaces (spec §5.2).
pub mod interfaces {
    pub use rsgdll_core::interface::{create_interface, lua_shared};
}

#[cfg(feature = "serde")]
pub use rsgdll_core::conv::serde::LuaSerde;

/// Runtime paths used by generated code. Not a public API.
#[doc(hidden)]
pub mod __rt {
    pub use rsgdll_core::*;
    pub use rsgdll_sys as sys;
    pub use linkme;
    pub use rsgdll_gmod as gmod;
}

pub mod prelude {
    pub use crate::{
        Angle, Color, Context, Entity, EntityRef, Error, ErrorKind, Lua, LuaBytes, LuaFunction,
        LuaTable, LuaUd, LuaUserData, LuaValue, Nil, Player, PlayerRef, Result, SteamId64,
        Variadic, Vector, anyhow, lua, LocalLua, LuaHandle, Callback,
    };
}
