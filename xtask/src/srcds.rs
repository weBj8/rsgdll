//! `test-srcds` (spec §12): build examples, install them + their autorun test scripts into
//! the configured srcds, run it, and assert `RSGDLL_TEST` lines + `RSGDLL_TEST_DONE`.

use std::path::PathBuf;
use std::time::Duration;

use crate::build::{self, Platform, RealmArg, Spec};
use crate::config;

/// (crate, Lua module name) of every example built and run.
const EXAMPLES: &[(&str, &str)] = &[("minimal", "minimal"), ("showcase", "showcase")];

fn examples() -> Vec<(&'static str, &'static str)> {
    let root = config::workspace_root();
    EXAMPLES.iter().copied().filter(|(c, _)| root.join("examples").join(c).exists()).collect()
}

/// Test scripts of an example (`examples/<c>/lua/autorun/server/*.lua`).
fn scripts(krate: &str) -> Vec<PathBuf> {
    let dir = config::workspace_root().join("examples").join(krate).join("lua/autorun/server");
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "lua")).collect())
        .unwrap_or_default()
}

pub fn test(keep: bool) -> Result<(), String> {
    let srcds = config::srcds_dir()?;
    let autorun = srcds.join("garrysmod/lua/autorun/server");
    let list = examples();
    let installed: Vec<PathBuf> = list
        .iter()
        .map(|(krate, module)| {
            let spec = Spec {
                krate: (*krate).into(),
                module: (*module).into(),
                platform: Platform::Linux64,
                realm: RealmArg::Sv,
                release: true,
                menu: false,
                extra_features: Vec::new(),
                native: true,
            };
            build::build(&spec).and_then(|f| build::install(&f, RealmArg::Sv))
        })
        .collect::<Result<_, _>>()?;
    let copied: Vec<PathBuf> = list
        .iter()
        .flat_map(|(k, _)| scripts(k))
        .map(|src| {
            let dest = autorun.join(format!("rsgdll_{}", src.file_name().and_then(|n| n.to_str()).unwrap_or("t.lua")));
            std::fs::copy(&src, &dest).map(|_| dest).map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    let run = build::run_srcds(Duration::from_secs(120), copied.len(), &[]);
    if !keep {
        copied.iter().chain(&installed).for_each(|p| drop(std::fs::remove_file(p)));
    }
    let run = run?;
    let log = config::workspace_root().join("target/srcds-test.log");
    std::fs::write(&log, run.log.join("\n")).map_err(|e| e.to_string())?;
    run.results.iter().for_each(|(n, ok, d)| println!("{} {n} {d}", if *ok { "PASS" } else { "FAIL" }));
    let failed = run.results.iter().filter(|r| !r.1).count();
    match (run.done, failed, run.results.is_empty()) {
        (true, 0, false) => {
            println!("all {} srcds tests passed (log: {})", run.results.len(), log.display());
            Ok(())
        }
        (false, _, _) => Err(format!("timeout: RSGDLL_TEST_DONE not seen for every script (log: {})", log.display())),
        (_, _, true) => Err(format!("no RSGDLL_TEST results (log: {})", log.display())),
        _ => Err(format!("{failed} srcds test(s) failed (log: {})", log.display())),
    }
}
