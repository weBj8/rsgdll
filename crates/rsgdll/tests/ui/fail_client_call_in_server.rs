use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::function]
fn hud(lua: &Lua) -> Result<i32> {
    lua!(lua, return ScrW())
}

fn main() {}
