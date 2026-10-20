//! Safe wrappers for GMod Lua libraries (spec §10.3/10.4): hook, timer, concommand,
//! convars, net helpers, typed built-in hooks.

pub mod hook;
#[rustfmt::skip]
pub mod hooks_gen;
pub mod misc;
pub mod timer;

/// Typed built-in hook markers (`hooks::PlayerSay`, `hooks::sandbox::...`).
pub mod hooks {
    pub use crate::hooks_gen::*;
}

#[doc(hidden)]
pub use rsgdll_core::Realm as __Realm;
pub use misc::{concommand, cvars, error_no_halt, msg, net, print};

use linkme::distributed_slice;
use rsgdll_core::Lua;
use rsgdll_core::module::{Ordered, TEARDOWN};

#[distributed_slice(TEARDOWN)]
static CONCOMMANDS: Ordered<fn(&Lua)> = Ordered { order: 1, name: "rsgdll.concommands", f: concommand::teardown };
