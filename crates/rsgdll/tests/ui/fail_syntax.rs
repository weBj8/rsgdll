use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::function]
fn bad(lua: &Lua) -> Result<()> {
    lua!(lua, r#"local x = = 1"#)
}

fn main() {}
