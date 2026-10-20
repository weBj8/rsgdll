//! Async and multithreading (spec §10): main-thread executor, `LuaHandle`, `LocalLua`,
//! worker pool, optional tokio runtime, and the pump driver registered at open.

pub mod executor;
pub mod handle;
pub mod local;
pub mod threaded;
#[cfg(feature = "tokio")]
pub mod tokio_rt;
pub mod worker;

use std::time::Duration;

use linkme::distributed_slice;

pub use executor::{sleep, spawn_local};
pub use handle::LuaHandle;
pub use local::LocalLua;
pub use worker::spawn_worker;

use crate::conv::LuaFunction;
use crate::error::Result;
use crate::lua::Lua;
use crate::module::{Ordered, REGISTER, TEARDOWN};

/// Worker shutdown wait at close (spec §8.1).
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(2);

fn pump_id(lua: &Lua) -> String {
    format!("rsgdll.{}.pump", lua.module_name())
}

/// Registers the pump: `Think` (server/client) or `Tick`+`DrawOverlay` (menu). Skipped
/// outside GMod (no `hook` library); tests pump manually.
fn start_pump(lua: &Lua) -> Result<()> {
    let Some(add) = lua.global::<Option<LuaFunction>>("hook.Add")? else {
        return Ok(());
    };
    executor::get(lua);
    let events: &[&str] = match lua.is_menu() {
        true => &["Tick", "DrawOverlay"],
        false => &["Think"],
    };
    let id = pump_id(lua);
    events.iter().try_for_each(|ev| {
        let f = lua.create_function(|lua, ()| {
            executor::pump(lua);
            Ok(())
        })?;
        add.call::<_, ()>(lua, (*ev, id.as_str(), f))?;
        crate::tracked::hook(lua, ev, &id);
        Ok(())
    })
}

#[distributed_slice(REGISTER)]
static START_PUMP: Ordered<fn(&Lua) -> Result<()>> = Ordered {
    order: i32::MAX,
    name: "rsgdll.pump",
    f: start_pump,
};

fn stop_all(lua: &Lua) {
    executor::stop(lua, WORKER_TIMEOUT);
}

#[distributed_slice(TEARDOWN)]
static STOP: Ordered<fn(&Lua)> = Ordered {
    order: 0,
    name: "rsgdll.executor",
    f: stop_all,
};
