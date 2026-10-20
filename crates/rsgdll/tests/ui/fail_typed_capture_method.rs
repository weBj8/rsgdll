use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::function]
fn nick(lua: &Lua, p: PlayerRef<'_>) -> Result<String> {
    lua!(lua, "return $ply:Nikc()", ply: PlayerRef = p)
}

fn main() {}
