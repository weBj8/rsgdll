//! `build`, `install`, `test-srcds` (spec §11, §12).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Platform {
    Win32,
    Win64,
    Linux,
    Linux64,
}

impl Platform {
    pub const ALL: [Platform; 4] = [
        Platform::Win32,
        Platform::Win64,
        Platform::Linux,
        Platform::Linux64,
    ];

    pub fn parse(s: &str) -> Option<Vec<Platform>> {
        Some(match s {
            "all" => Self::ALL.to_vec(),
            "win32" => vec![Platform::Win32],
            "win64" => vec![Platform::Win64],
            "linux" => vec![Platform::Linux],
            "linux64" => vec![Platform::Linux64],
            _ => return None,
        })
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Platform::Win32 => "win32",
            Platform::Win64 => "win64",
            Platform::Linux => "linux",
            Platform::Linux64 => "linux64",
        }
    }

    pub fn triple(self) -> &'static str {
        match self {
            Platform::Win32 => "i686-pc-windows-msvc",
            Platform::Win64 => "x86_64-pc-windows-msvc",
            Platform::Linux => "i686-unknown-linux-gnu",
            Platform::Linux64 => "x86_64-unknown-linux-gnu",
        }
    }

    /// Cargo subcommand: zigbuild with an old glibc baseline on Linux, xwin on Windows.
    fn cargo(self, native: bool) -> (Vec<String>, String) {
        match (self, native) {
            (Platform::Linux64, true) => (vec!["build".into()], self.triple().into()),
            (Platform::Linux | Platform::Linux64, _) => {
                (vec!["zigbuild".into()], format!("{}.2.17", self.triple()))
            }
            (Platform::Win32 | Platform::Win64, _) => {
                (vec!["xwin".into(), "build".into()], self.triple().into())
            }
        }
    }

    fn artifact(self, krate: &str) -> String {
        match self {
            Platform::Win32 | Platform::Win64 => format!("{krate}.dll"),
            _ => format!("lib{krate}.so"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RealmArg {
    Sv,
    Cl,
}

impl RealmArg {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sv" => Some(RealmArg::Sv),
            "cl" => Some(RealmArg::Cl),
            _ => None,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            RealmArg::Sv => "gmsv",
            RealmArg::Cl => "gmcl",
        }
    }
    fn features(self, menu: bool) -> String {
        match (self, menu) {
            (RealmArg::Sv, _) => "server".into(),
            (RealmArg::Cl, false) => "client".into(),
            (RealmArg::Cl, true) => "client,menu".into(),
        }
    }
}

pub struct Spec {
    pub krate: String,
    /// Lua module name (the `module!(name = ...)`), used for the file name.
    pub module: String,
    pub platform: Platform,
    pub realm: RealmArg,
    pub release: bool,
    pub menu: bool,
    pub extra_features: Vec<String>,
    /// Use plain `cargo build` for the host target (local tests).
    pub native: bool,
}

/// Builds one cdylib (each realm in its own cargo invocation) and returns the renamed file.
pub fn build(s: &Spec) -> Result<PathBuf, String> {
    let root = config::workspace_root();
    let (sub, target) = s.platform.cargo(s.native);
    let features = std::iter::once(s.realm.features(s.menu))
        .chain(s.extra_features.iter().cloned())
        .collect::<Vec<_>>()
        .join(",");
    // A per-realm target dir keeps feature sets from invalidating each other.
    let target_dir = root
        .join("target")
        .join(format!("realm-{}", s.realm.prefix()));
    let mut cmd = Command::new("cargo");
    if !s.native {
        // Cross linkers (zig, lld-link) reject host-specific link args from a user's global
        // `build.rustflags` (e.g. mold); RUSTFLAGS takes precedence over config rustflags.
        cmd.env("RUSTFLAGS", "");
        // cargo-xwin only fetches x86_64/aarch64 SDK libs by default; win32 needs x86.
        cmd.env("XWIN_ARCH", "x86,x86_64");
    }
    let status = cmd
        .args(&sub)
        .args([
            "-p",
            &s.krate,
            "--target",
            &target,
            "--no-default-features",
            "--features",
            &features,
        ])
        .args(s.release.then_some("--release"))
        .arg("--target-dir")
        .arg(&target_dir)
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo {}: {e}", sub.join(" ")))?;
    if !status.success() {
        return Err(format!(
            "build of {} for {} failed",
            s.krate,
            s.platform.suffix()
        ));
    }
    let triple_dir = target.split(".2.").next().unwrap_or(&target).to_owned();
    let profile = if s.release { "release" } else { "debug" };
    let built = target_dir
        .join(triple_dir)
        .join(profile)
        .join(s.platform.artifact(&s.krate));
    let out_dir = root.join("target/gmod");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out = out_dir.join(format!(
        "{}_{}_{}.dll",
        s.realm.prefix(),
        s.module,
        s.platform.suffix()
    ));
    std::fs::copy(&built, &out).map_err(|e| format!("{}: {e}", built.display()))?;
    Ok(out)
}

/// Copies a built module into `<game>/garrysmod/lua/bin`.
pub fn install(file: &Path, realm: RealmArg) -> Result<PathBuf, String> {
    let game = match realm {
        RealmArg::Sv => config::srcds_dir()?,
        RealmArg::Cl => config::gmod_client_dir()?,
    };
    let bin = game.join("garrysmod/lua/bin");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let dest = bin.join(file.file_name().ok_or("bad file name")?);
    std::fs::copy(file, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
}

/// Result of one srcds run.
pub struct Run {
    pub results: Vec<(String, bool, String)>,
    pub done: bool,
    pub log: Vec<String>,
}

/// Parses `RSGDLL_TEST <name> PASS|FAIL <detail>` lines.
pub fn parse_line(line: &str) -> Option<(String, bool, String)> {
    let rest = line.split("RSGDLL_TEST ").nth(1)?;
    let mut it = rest.splitn(3, ' ');
    let name = it.next()?.to_owned();
    let pass = match it.next()? {
        "PASS" => true,
        "FAIL" => false,
        _ => return None,
    };
    Some((name, pass, it.next().unwrap_or("").trim().to_owned()))
}

/// Runs srcds with the installed test scripts until `RSGDLL_TEST_DONE` or timeout.
/// `extra` = additional console args (e.g. `+changelevel` driven by the test itself).
pub fn run_srcds(
    timeout: Duration,
    done_marker_count: usize,
    extra: &[&str],
) -> Result<Run, String> {
    let dir = config::srcds_dir()?;
    // stdout to a pipe is block-buffered: with little console output the DONE markers never flush
    let mut child = Command::new("stdbuf")
        .arg("-oL")
        .arg(dir.join("srcds_run_x64"))
        .args([
            "-game",
            "garrysmod",
            "-norestart",
            // secure Steam init hangs on CI runners (no Steam auth); tests need no VAC
            "-insecure",
            "-port",
            "27199",
            "+maxplayers",
            "2",
            "+map",
            "gm_construct",
            "+sv_hibernate_think",
            "1",
        ])
        .args(extra)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("srcds_run_x64: {e}"))?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| drop(tx.send(l)))
    });
    // tier0 Msg/Warning and early-load failures can go to stderr; keep them in the log.
    std::thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| drop(tx2.send(format!("[stderr] {l}"))))
    });
    let start = Instant::now();
    let mut run = Run {
        results: Vec::new(),
        done: false,
        log: Vec::new(),
    };
    // Distinct `RSGDLL_TEST_DONE <script>` markers (a reload test re-runs other scripts).
    let mut dones: std::collections::HashSet<String> = std::collections::HashSet::new();
    while start.elapsed() < timeout && dones.len() < done_marker_count {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            if let Some(r) = parse_line(&line) {
                run.results.push(r);
            }
            if let Some(rest) = line.split("RSGDLL_TEST_DONE").nth(1) {
                dones.insert(rest.trim().to_owned());
            }
            run.log.push(line);
        }
    }
    run.done = dones.len() >= done_marker_count;
    // Let shutdown messages (close hooks) flush, then stop the server.
    let grace = Instant::now();
    while grace.elapsed() < Duration::from_secs(3) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            run.log.push(line);
        }
    }
    // record whether srcds died on its own (crash/exit) before we kill it
    run.log.push(format!(
        "[xtask] srcds status before kill: {:?}",
        child.try_wait()
    ));
    let _ = child.kill();
    let _ = child.wait();
    rx.try_iter().for_each(|l| run.log.push(l));
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_lines() {
        assert_eq!(
            parse_line("RSGDLL_TEST add PASS "),
            Some(("add".into(), true, "".into()))
        );
        assert_eq!(
            parse_line("x RSGDLL_TEST e FAIL boom here"),
            Some(("e".into(), false, "boom here".into()))
        );
        assert_eq!(parse_line("nothing"), None);
        assert_eq!(Platform::Linux64.artifact("m"), "libm.so");
        assert_eq!(Platform::Win32.cargo(false).1, "i686-pc-windows-msvc");
    }
}
