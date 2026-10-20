//! rsgdll developer tasks. Run with `cargo xtask <command>`.

mod config;
mod probe;

use std::path::PathBuf;

fn usage() -> String {
    [
        "usage: cargo xtask <command>",
        "  probe-luajit [lua_shared path...]   record LuaJIT version, symbols, unwinding, GLua syntax",
    ]
    .join("\n")
}

/// Default probe inputs: every `lua_shared` under the configured srcds dir.
fn default_probe_files() -> Result<Vec<PathBuf>, String> {
    let dir = config::srcds_dir()?;
    Ok(["bin/linux64/lua_shared.so", "bin/lua_shared.so", "garrysmod/bin/lua_shared_srv.so",
        "bin/win64/lua_shared.dll", "bin/lua_shared.dll"]
        .iter()
        .map(|p| dir.join(p))
        .filter(|p| p.exists())
        .collect())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((cmd, rest)) if cmd == "probe-luajit" => {
            let files = match rest {
                [] => default_probe_files()?,
                r => r.iter().map(PathBuf::from).collect(),
            };
            files
                .iter()
                .map(|f| probe::probe(f).map(|r| probe::render(&r)))
                .try_for_each(|r| r.map(|s| println!("{s}\n")))
        }
        _ => Err(usage()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
