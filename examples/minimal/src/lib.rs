//! Smallest complete rsgdll module (spec §13.1); the template users copy.

use rsgdll::prelude::*;

rsgdll::module!(name = "minimal");

#[rsgdll::postopen]
fn open(lua: &Lua) -> Result<()> {
    lua.print("minimal: open")
}

#[rsgdll::preclose]
fn close(lua: &Lua) {
    let _ = lua.print("minimal: close");
}

/// Lua: `minimal.Add(1, 2) -> 3`
#[rsgdll::function]
fn add(a: i64, b: i64) -> i64 {
    a + b
}

/// Lua: `minimal.Divide(1, 0)` raises a Lua error.
#[rsgdll::function]
fn divide(a: f64, b: f64) -> Result<f64> {
    (b != 0.0).then(|| a / b).ok_or_else(|| rsgdll::anyhow!("division by zero"))
}

/// Rust -> Lua -> Rust.
#[rsgdll::function]
fn greet(lua: &Lua, name: String) -> Result<String> {
    let host: String = lua!(lua, GetHostName())?;
    Ok(format!("hello {name} from {host}"))
}
