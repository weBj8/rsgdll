use rsgdll::prelude::*;
rsgdll::module!(name = "uitest");

#[rsgdll::hook(HUDPaint)]
fn paint() -> Result<()> { Ok(()) }

fn main() {}
