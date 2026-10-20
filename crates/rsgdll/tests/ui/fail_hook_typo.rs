use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::hook(PlayerSya)]
fn on_say(text: String) -> Result<()> { let _ = text; Ok(()) }

fn main() {}
