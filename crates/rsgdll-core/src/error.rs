//! Error model (spec §7.1): anyhow-style `Error { kind, context, backtrace }`.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Cheap conversion error with a field/index path; no backtrace (spec §7.1).
#[derive(Clone, Debug, PartialEq)]
pub struct ConvError {
    pub expected: String,
    pub got: String,
    /// Innermost last: `["Retries"]`, `["3"]`.
    pub path: Vec<String>,
}

impl ConvError {
    pub fn new(expected: impl Into<String>, got: impl Into<String>) -> Self {
        Self { expected: expected.into(), got: got.into(), path: Vec::new() }
    }

    /// Message without "got" (e.g. `expected sequence, hole at 3`).
    pub fn msg(text: impl Into<String>) -> Self {
        Self::new(text, "")
    }

    /// Prepends an outer path segment.
    pub fn at(self, seg: impl Into<String>) -> Self {
        Self { path: std::iter::once(seg.into()).chain(self.path).collect(), ..self }
    }
}

impl fmt::Display for ConvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match self.got.is_empty() {
            true => self.expected.clone(),
            false => format!("expected {}, got {}", self.expected, self.got),
        };
        match self.path.is_empty() {
            true => f.write_str(&body),
            false => write!(f, "field '{}': {body}", self.path.join(".")),
        }
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum ErrorKind {
    Lua { message: String, lua_traceback: Option<String> },
    StateClosed,
    Conversion(ConvError),
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Lua { message, lua_traceback: Some(tb) } => write!(f, "{message}\n{tb}"),
            ErrorKind::Lua { message, .. } => f.write_str(message),
            ErrorKind::StateClosed => f.write_str("Lua state closed"),
            ErrorKind::Conversion(c) => c.fmt(f),
            ErrorKind::Other(e) => e.fmt(f),
        }
    }
}

pub struct Error {
    kind: ErrorKind,
    context: Vec<String>,
    backtrace: Option<Box<Backtrace>>,
}

// 0 = default (debug_assertions || feature), 1 = forced off, 2 = forced on.
static BACKTRACE: AtomicU8 = AtomicU8::new(0);

/// Runtime override of backtrace capture for this DLL (spec §7.1).
pub fn set_backtrace(on: bool) {
    BACKTRACE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
}

pub fn backtrace_enabled() -> bool {
    match BACKTRACE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => cfg!(any(debug_assertions, feature = "backtrace")),
    }
}

fn capture() -> Option<Box<Backtrace>> {
    backtrace_enabled().then(|| Box::new(Backtrace::force_capture()))
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, context: Vec::new(), backtrace: capture() }
    }

    pub fn msg(text: impl fmt::Display) -> Self {
        Self::new(ErrorKind::Other(text.to_string().into()))
    }

    pub fn lua(message: impl Into<String>, lua_traceback: Option<String>) -> Self {
        Self::new(ErrorKind::Lua { message: message.into(), lua_traceback })
    }

    /// Error from already formatted text (no backtrace is added).
    pub fn preformatted(text: String) -> Self {
        Self { kind: ErrorKind::Other(text.into()), context: Vec::new(), backtrace: None }
    }

    pub fn state_closed() -> Self {
        Self::new(ErrorKind::StateClosed)
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn backtrace(&self) -> Option<&Backtrace> {
        self.backtrace.as_deref().filter(|b| b.status() == BacktraceStatus::Captured)
    }

    pub fn context(mut self, c: impl fmt::Display) -> Self {
        self.context.push(c.to_string());
        self
    }

    /// Interop with `anyhow`, `thiserror` `#[from]` etc.
    pub fn into_std(self) -> Box<dyn std::error::Error + Send + Sync> {
        match (self.kind, self.context.is_empty()) {
            (ErrorKind::Other(e), true) => e,
            (kind, _) => Error { kind, ..self }.to_string().into(),
        }
    }

    /// Message without backtrace (contexts outermost first, then `caused by:`).
    pub fn message(&self) -> String {
        self.context
            .iter()
            .rev()
            .map(String::as_str)
            .map(str::to_owned)
            .chain(std::iter::once(self.kind.to_string()))
            .enumerate()
            .map(|(i, s)| if i == 0 { s } else { format!("caused by: {s}") })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Full message as Lua sees it: message + trimmed `rust stack traceback` (spec §7.2).
    pub fn lua_message(&self) -> String {
        match self.backtrace() {
            Some(bt) => format!("{}\nrust stack traceback:\n{}", self.message(), trim_backtrace(&bt.to_string())),
            None => self.message(),
        }
    }
}

/// Drops std/rsgdll/runtime frames; keeps user frames with their `at file:line` lines.
pub fn trim_backtrace(bt: &str) -> String {
    const SKIP: &[&str] = &["std::", "core::", "alloc::", "rsgdll", "__rust", "backtrace::", "<unknown>", "__libc", "_start", "clone3", "start_thread"];
    let frames: Vec<Vec<&str>> = bt.lines().fold(Vec::new(), |mut acc: Vec<Vec<&str>>, line| {
        let t = line.trim_start();
        match t.split_once(": ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())) {
            Some(_) => acc.push(vec![t]),
            None => if let Some(f) = acc.last_mut() { f.push(t) },
        }
        acc
    });
    frames
        .iter()
        .filter(|f| f.first().and_then(|h| h.split_once(": ")).is_some_and(|(_, name)| !SKIP.iter().any(|s| name.contains(s))))
        .enumerate()
        .map(|(i, f)| {
            let name = f.first().and_then(|h| h.split_once(": ")).map_or("", |(_, n)| n);
            let at = f.iter().skip(1).map(|l| format!(" {l}")).collect::<String>();
            format!("    {i}: {name}{at}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lua_message())
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(e: E) -> Self {
        Self::new(ErrorKind::Other(Box::new(e)))
    }
}

impl From<ConvError> for Error {
    fn from(e: ConvError) -> Self {
        Self::new(ErrorKind::Conversion(e))
    }
}

/// `.context(...)` on `Result`/`Option`.
pub trait Context<T> {
    fn context(self, c: impl fmt::Display) -> Result<T>;
    fn with_context<C: fmt::Display>(self, f: impl FnOnce() -> C) -> Result<T>;
}

impl<T, E: Into<Error>> Context<T> for std::result::Result<T, E> {
    fn context(self, c: impl fmt::Display) -> Result<T> {
        self.map_err(|e| e.into().context(c))
    }
    fn with_context<C: fmt::Display>(self, f: impl FnOnce() -> C) -> Result<T> {
        self.map_err(|e| e.into().context(f()))
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, c: impl fmt::Display) -> Result<T> {
        self.ok_or_else(|| Error::msg(c))
    }
    fn with_context<C: fmt::Display>(self, f: impl FnOnce() -> C) -> Result<T> {
        self.ok_or_else(|| Error::msg(f()))
    }
}

/// Builds an `ErrorKind::Other` error from a format string.
#[macro_export]
macro_rules! anyhow {
    ($($t:tt)*) => { $crate::error::Error::msg(format!($($t)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_display_and_kind() {
        let e: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
        let e = e.context("failed to read config.json");
        assert_eq!(e.message(), "failed to read config.json\ncaused by: nope");
        assert!(matches!(e.kind(), ErrorKind::Other(_)));
        let conv: Error = ConvError::new("number", "string").at("Retries").into();
        assert_eq!(conv.to_string(), "field 'Retries': expected number, got string");
    }

    #[test]
    fn backtrace_switch() {
        set_backtrace(true);
        assert!(anyhow!("x").backtrace().is_some());
        set_backtrace(false);
        assert!(anyhow!("x").backtrace().is_none());
    }

    #[test]
    fn trims_frames() {
        let bt = "   0: std::backtrace::Backtrace::create\n             at /rustc/x\n   1: mymod::load_config\n             at src/lib.rs:42:5\n   2: core::ops::function::FnOnce::call_once";
        assert_eq!(trim_backtrace(bt), "    0: mymod::load_config at src/lib.rs:42:5");
    }
}
