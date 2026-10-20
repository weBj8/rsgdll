#![forbid(unsafe_code)]
use rsgdll::prelude::*;

rsgdll::module!(name = "uitest");

#[rsgdll::function]
fn add(a: i64, b: i64) -> i64 { a + b }

#[rsgdll::function]
fn greet(lua: &Lua, name: String) -> Result<String> {
    let host: String = lua!(lua, GetHostName())?;
    Ok(format!("hello {name} from {host}"))
}

fn main() {}
