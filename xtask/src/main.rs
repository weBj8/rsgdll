//! rsgdll developer tasks. Run with `cargo xtask <command>`.

mod apidb;
mod build;
mod config;
mod hooks;
mod probe;
mod srcds;
mod vtables;

use std::path::PathBuf;

fn usage() -> String {
    [
        "usage: cargo xtask <command>",
        "  probe-luajit [lua_shared path...]   record LuaJIT version, symbols, unwinding, GLua syntax",
        "  build [--target all|win32|win64|linux|linux64] [--realm sv|cl] [--release] [--menu] -p <crate>",
        "  install --realm sv|cl <file>        copy a built module into garrysmod/lua/bin",
        "  test-srcds [--keep]                 build+install examples, run srcds, assert RSGDLL_TEST lines",
        "  gen-apidb [library dir]             regenerate crates/rsgdll-apidb/src/data.txt",
        "  gen-vtables [headers dir]           regenerate crates/rsgdll-sys/src/vtables.rs",
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
        Some((cmd, rest)) if cmd == "gen-apidb" => {
            let root = config::workspace_root();
            let dir = match rest.first() {
                Some(d) => PathBuf::from(d),
                None => apidb::fetch(&root)?,
            };
            let out = root.join("crates/rsgdll-apidb/src");
            apidb::generate(&dir)
                .and_then(|data| std::fs::write(out.join("data.txt"), data).map_err(|e| e.to_string()))
                .and_then(|()| std::fs::write(out.join("commit.txt"), apidb::COMMIT).map_err(|e| e.to_string()))
                .and_then(|()| {
                    // Re-parse the fresh data so hook markers match it exactly.
                    let data = std::fs::read_to_string(out.join("data.txt")).map_err(|e| e.to_string())?;
                    let hooks_file = root.join("crates/rsgdll-gmod/src/hooks_gen.rs");
                    std::fs::write(&hooks_file, hooks::generate(&rsgdll_apidb::parse(&data))).map_err(|e| e.to_string())
                })
                .map(|()| println!("wrote {} and hooks_gen.rs", out.join("data.txt").display()))
        }
        Some((cmd, rest)) if cmd == "gen-vtables" => {
            let root = config::workspace_root();
            let dir = match rest.first() {
                Some(d) => PathBuf::from(d),
                None => vtables::fetch(&root)?,
            };
            let out = root.join("crates/rsgdll-sys/src/vtables.rs");
            vtables::generate(&dir)
                .and_then(|src| std::fs::write(&out, src).map_err(|e| e.to_string()))
                .map(|()| println!("wrote {}", out.display()))
        }
        Some((cmd, rest)) if cmd == "build" => {
            let opt = |k: &str| rest.iter().position(|a| a == k).and_then(|i| rest.get(i + 1)).map(String::as_str);
            let platforms = build::Platform::parse(opt("--target").unwrap_or("linux64")).ok_or("bad --target")?;
            let realm = build::RealmArg::parse(opt("--realm").unwrap_or("sv")).ok_or("bad --realm")?;
            let krate = opt("-p").ok_or("missing -p <crate>")?.to_owned();
            let module = opt("--name").unwrap_or(&krate).to_owned();
            platforms.iter().try_for_each(|p| {
                let spec = build::Spec {
                    krate: krate.clone(),
                    module: module.clone(),
                    platform: *p,
                    realm,
                    release: rest.iter().any(|a| a == "--release"),
                    menu: rest.iter().any(|a| a == "--menu"),
                    extra_features: Vec::new(),
                    native: false,
                };
                build::build(&spec).map(|f| println!("built {}", f.display()))
            })
        }
        Some((cmd, rest)) if cmd == "install" => {
            let realm = rest.iter().position(|a| a == "--realm").and_then(|i| rest.get(i + 1)).and_then(|r| build::RealmArg::parse(r)).ok_or("missing --realm sv|cl")?;
            let file = rest.last().filter(|f| !f.starts_with("--") && *f != "sv" && *f != "cl").ok_or("missing <file>")?;
            build::install(std::path::Path::new(file), realm).map(|d| println!("installed {}", d.display()))
        }
        Some((cmd, rest)) if cmd == "test-srcds" => srcds::test(rest.iter().any(|a| a == "--keep")),
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
