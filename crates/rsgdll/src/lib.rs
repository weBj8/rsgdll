//! rsgdll: Garry's Mod binary modules in plain Rust. `use rsgdll::prelude::*;`

pub use rsgdll_core::{Context, ConvError, Error, ErrorKind, Lua, Result, anyhow, set_backtrace};
pub use rsgdll_core::conv::*;
pub use rsgdll_macros::{LuaUserData, LuaValue};

#[cfg(feature = "serde")]
pub use rsgdll_core::conv::serde::LuaSerde;

/// Runtime paths used by generated code. Not a public API.
#[doc(hidden)]
pub mod __rt {
    pub use rsgdll_core::*;
    pub use rsgdll_sys as sys;
    pub use linkme;
}

pub mod prelude {
    pub use crate::{
        Angle, Color, Context, Entity, EntityRef, Error, ErrorKind, Lua, LuaBytes, LuaFunction,
        LuaTable, LuaUd, LuaUserData, LuaValue, Nil, Player, PlayerRef, Result, SteamId64,
        Variadic, Vector, anyhow,
    };
}
