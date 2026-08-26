//! Repository build tasks.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// GMod process that loads a binary module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Realm {
    Server,
    Client,
}

impl Realm {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "server" => Ok(Self::Server),
            "client" => Ok(Self::Client),
            _ => Err(format!("unsupported GMod realm: {value}")),
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Server => "gmsv",
            Self::Client => "gmcl",
        }
    }
}

/// Returns the filename expected by GMod's native module loader.
pub fn artifact_name(realm: Realm, module: &str, target: &str) -> Result<String, String> {
    if module.is_empty()
        || !module
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("invalid GMod module name: {module}"));
    }
    let suffix = match target {
        "i686-unknown-linux-gnu" => "linux",
        "x86_64-unknown-linux-gnu" => "linux64",
        "i686-pc-windows-msvc" | "i686-pc-windows-gnu" => "win32",
        "x86_64-pc-windows-msvc" | "x86_64-pc-windows-gnu" => "win64",
        _ => return Err(format!("unsupported GMod artifact target: {target}")),
    };
    Ok(format!("{}_{}_{}.dll", realm.prefix(), module, suffix))
}

/// Copies a built library into a GMod `lua/bin` directory.
pub fn stage_artifact(
    realm: Realm,
    module: &str,
    target: &str,
    source: &Path,
    destination: &Path,
) -> Result<PathBuf, StageError> {
    let filename = artifact_name(realm, module, target).map_err(StageError::Input)?;
    std::fs::create_dir_all(destination).map_err(StageError::Io)?;
    let staged = destination.join(filename);
    std::fs::copy(source, &staged).map_err(StageError::Io)?;
    Ok(staged)
}

#[derive(Debug)]
pub enum StageError {
    Input(String),
    Io(std::io::Error),
}

impl fmt::Display for StageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(message) => formatter.write_str(message),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl Error for StageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(_) => None,
            Self::Io(error) => Some(error),
        }
    }
}
