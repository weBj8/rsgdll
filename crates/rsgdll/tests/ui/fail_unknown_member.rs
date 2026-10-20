use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::function]
fn send(lua: &Lua) -> Result<()> {
    lua!(lua, net.WriteStrng("x"))
}

fn main() {}
