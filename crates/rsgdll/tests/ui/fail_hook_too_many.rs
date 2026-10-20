use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::hook(PlayerSay)]
fn on_say(_p: Option<PlayerRef<'_>>, _t: String, _team: bool, _extra: bool) -> Result<()> { Ok(()) }

fn main() {}
