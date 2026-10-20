//! Local paths from `xtask.toml` (gitignored) or env vars (spec §12). No hard-coded defaults.

use std::path::PathBuf;

#[derive(serde::Deserialize, Default)]
struct File {
    srcds_dir: Option<PathBuf>,
    gmod_client_dir: Option<PathBuf>,
}

fn file() -> File {
    std::fs::read_to_string(workspace_root().join("xtask.toml"))
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn lookup(env: &str, key: &str, pick: fn(File) -> Option<PathBuf>) -> Result<PathBuf, String> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .or_else(|| pick(file()))
        .ok_or_else(|| format!("missing `{key}` in xtask.toml (or env {env})"))
}

pub fn srcds_dir() -> Result<PathBuf, String> {
    lookup("RSGDLL_SRCDS_DIR", "srcds_dir", |f| f.srcds_dir)
}

#[allow(dead_code)] // used by `install --realm cl`
pub fn gmod_client_dir() -> Result<PathBuf, String> {
    lookup("RSGDLL_GMOD_CLIENT_DIR", "gmod_client_dir", |f| f.gmod_client_dir)
}
