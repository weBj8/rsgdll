use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::hook(PlayerSay)]
fn on_say(_ply: Option<PlayerRef<'_>>, text: i32) -> Result<()> { let _ = text; Ok(()) }

fn main() {}
