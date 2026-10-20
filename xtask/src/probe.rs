//! `probe-luajit`: record LuaJIT version, exported API symbols, unwinding mode and
//! GLua syntax acceptance of a `lua_shared` binary (spec §3, §15).

use std::ffi::{CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

/// GLua snippets that must load with `luaL_loadbuffer` (spec §8.3).
const GLUA_SNIPPETS: &[(&str, &str)] = &[
    ("neq", "return 1 != 2"),
    ("not", "return !true"),
    ("and_or", "return true && false || true"),
    ("line_comment", "// c\nreturn 1"),
    ("block_comment", "/* c */ return 1"),
    ("continue", "for i=1,2 do if i==1 then continue end end"),
    ("goto", "do goto a end ::a::"),
];

pub struct Report {
    pub file: PathBuf,
    pub version: Option<String>,
    pub symbols: Vec<String>,
    pub unwinding: &'static str,
    pub glua: Vec<(&'static str, bool)>,
}

/// Null-terminated ASCII identifiers in a binary (export names live in the string table).
fn identifiers(bytes: &[u8]) -> impl Iterator<Item = &str> {
    bytes
        .split(|b| *b == 0)
        .filter_map(|s| std::str::from_utf8(s).ok())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

fn is_api(s: &str) -> bool {
    ["lua_", "luaL_", "luaJIT_"]
        .iter()
        .any(|p| s.starts_with(p))
}

fn unwinding(bytes: &[u8], is_pe: bool) -> &'static str {
    let has = |n: &str| identifiers(bytes).any(|s| s == n);
    match (is_pe, has("_Unwind_RaiseException")) {
        (true, _) => "seh (native exceptions)",
        (false, true) => "dwarf (native exceptions)",
        (false, false) => "longjmp (internal)",
    }
}

type NewState = unsafe extern "C-unwind" fn() -> *mut c_void;
type LoadBuffer =
    unsafe extern "C-unwind" fn(*mut c_void, *const c_char, usize, *const c_char) -> c_int;
type Close = unsafe extern "C-unwind" fn(*mut c_void);

/// Loads the library (after its sibling engine deps) and tries each GLua snippet.
fn glua_syntax(file: &Path) -> Result<Vec<(&'static str, bool)>, String> {
    let dir = file.parent().unwrap_or(Path::new("."));
    // Keep deps alive for the duration of the probe.
    let _deps: Vec<libloading::Library> = ["libtier0.so", "libvstdlib.so"]
        .iter()
        .map(|d| dir.join(d))
        .filter(|p| p.exists())
        .filter_map(|p| open_global(&p).ok())
        .collect();
    let lib = open_global(file)?;
    // SAFETY: symbols have the documented Lua 5.1 C API signatures.
    unsafe {
        let new: libloading::Symbol<NewState> =
            lib.get(b"luaL_newstate").map_err(|e| e.to_string())?;
        let load: libloading::Symbol<LoadBuffer> =
            lib.get(b"luaL_loadbuffer").map_err(|e| e.to_string())?;
        let close: libloading::Symbol<Close> = lib.get(b"lua_close").map_err(|e| e.to_string())?;
        let l = new();
        let name = CString::new("=probe").map_err(|e| e.to_string())?;
        let out = GLUA_SNIPPETS
            .iter()
            .map(|(n, src)| {
                (
                    *n,
                    load(l, src.as_ptr().cast(), src.len(), name.as_ptr()) == 0,
                )
            })
            .collect();
        close(l);
        Ok(out)
    }
}

#[cfg(unix)]
fn open_global(p: &Path) -> Result<libloading::Library, String> {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_NOW};
    // SAFETY: loading engine libraries runs their initialisers; acceptable for a probe tool.
    unsafe { Library::open(Some(p), RTLD_NOW | RTLD_GLOBAL) }
        .map(Into::into)
        .map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(windows)]
fn open_global(p: &Path) -> Result<libloading::Library, String> {
    // SAFETY: see unix variant.
    unsafe { libloading::Library::new(p) }.map_err(|e| format!("{}: {e}", p.display()))
}

pub fn probe(file: &Path) -> Result<Report, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let is_pe = bytes.starts_with(b"MZ");
    let mut symbols: Vec<String> = identifiers(&bytes)
        .filter(|s| is_api(s))
        .map(str::to_owned)
        .collect();
    symbols.sort();
    symbols.dedup();
    let version = symbols
        .iter()
        .find_map(|s| s.strip_prefix("luaJIT_version_"))
        .map(|v| v.replace('_', "."));
    let glua = glua_syntax(file).unwrap_or_else(|e| {
        eprintln!("glua syntax probe skipped: {e}");
        Vec::new()
    });
    Ok(Report {
        file: file.to_owned(),
        version,
        unwinding: unwinding(&bytes, is_pe),
        symbols,
        glua,
    })
}

pub fn render(r: &Report) -> String {
    [
        format!("file: {}", r.file.display()),
        format!("luajit: {}", r.version.as_deref().unwrap_or("unknown")),
        format!("unwinding: {}", r.unwinding),
        format!(
            "glua: {}",
            r.glua
                .iter()
                .map(|(n, ok)| format!("{n}={ok}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        format!("symbols ({}):", r.symbols.len()),
    ]
    .into_iter()
    .chain(r.symbols.iter().map(|s| format!("  {s}")))
    .collect::<Vec<_>>()
    .join("\n")
}
