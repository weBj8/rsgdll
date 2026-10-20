//! `gen-vtables`: transcribe garrysmod_common's pinned headers into Rust vtable structs
//! (spec §5.2). Applies the ABI rules: `-unwind` ABIs, variadics as `extern "C"`,
//! per-OS destructor slots, MSVC overload grouping, padding for C++-by-value members.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = "https://github.com/danielga/garrysmod_common";
pub const COMMIT: &str = "f77a18d";
const HEADERS: &[&str] = &["LuaBase.h", "LuaInterface.h", "LuaShared.h"];
/// Classes emitted, with the class they extend.
const CLASSES: &[(&str, Option<&str>)] = &[
    ("ILuaBase", None),
    ("ILuaThreadedCall", None),
    ("ILuaInterface", Some("ILuaBase")),
    ("ILuaShared", None),
];
/// Members that raise Lua errors and must never be called from Rust (spec §5.2).
const FORCED_PADDING: &[&str] = &["Error", "ErrorFromLua"];

#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    pub name: String,
    pub ret: String,
    pub params: Vec<String>,
    pub variadic: bool,
    pub destructor: bool,
}

#[derive(Clone, Debug)]
pub struct Class {
    pub name: String,
    pub methods: Vec<Method>,
}

/// Splits on top-level commas (function-pointer params contain nested parens).
fn split_top(s: &str) -> Vec<String> {
    let (mut out, cur, _) = s.chars().fold((Vec::new(), String::new(), 0i32), |(mut out, mut cur, depth), c| {
        match c {
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                (out, cur, depth)
            }
            '(' => { cur.push(c); (out, cur, depth + 1) }
            ')' => { cur.push(c); (out, cur, depth - 1) }
            _ => { cur.push(c); (out, cur, depth) }
        }
    });
    out.push(cur);
    out.into_iter().map(|p| p.trim().to_owned()).filter(|p| !p.is_empty() && p != "void").collect()
}

const TYPE_WORDS: &[&str] = &["int", "char", "bool", "double", "float", "long", "void", "unsigned", "const", "size_t"];

/// Drops default value and parameter name, normalising pointer spacing.
fn param_type(p: &str) -> String {
    let p = p.split('=').next().unwrap_or(p).trim();
    if p.contains('(') {
        return "fnptr".into();
    }
    let p = p.replace('*', " * ").replace('&', " & ");
    let toks: Vec<&str> = p.split_whitespace().collect();
    let keep = match toks.last() {
        Some(last) if toks.len() >= 2 && !TYPE_WORDS.contains(last) && !["*", "&"].contains(last) => toks.len() - 1,
        _ => toks.len(),
    };
    toks[..keep].join(" ").replace(" *", "*").replace(" &", "&")
}

/// Parses one `virtual ... ;` declaration.
pub fn parse_decl(decl: &str) -> Option<Method> {
    let body = decl.trim().strip_prefix("virtual")?.trim();
    let open = body.find('(')?;
    let close = body.rfind(')')?;
    let head = body[..open].trim().replace('*', "* ").replace('&', "& ");
    let mut toks: Vec<&str> = head.split_whitespace().collect();
    let name = toks.pop()?.to_owned();
    let params = split_top(&body[open + 1..close]);
    let variadic = params.last().is_some_and(|p| p == "...");
    Some(Method {
        destructor: name.starts_with('~'),
        name: name.trim_start_matches('~').to_owned(),
        ret: toks.join(" ").replace(" *", "*").replace(" &", "&").trim().to_owned(),
        params: params.iter().filter(|p| *p != "...").map(|p| param_type(p)).collect(),
        variadic,
    })
}

/// Collects classes and their own virtuals in declaration order.
pub fn parse_header(src: &str) -> Vec<Class> {
    let (mut classes, _) = src.lines().map(str::trim).fold(
        (Vec::<Class>::new(), None::<String>),
        |(mut classes, pending), line| {
            let line = line.split("//").next().unwrap_or("").trim();
            let pending = match pending {
                Some(p) => Some(format!("{p} {line}")),
                None if line.starts_with("virtual") => Some(line.to_owned()),
                None => {
                    if let Some(rest) = line.strip_prefix("class ")
                        && !rest.ends_with(';') {
                            let name = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
                            classes.push(Class { name: name.to_owned(), methods: Vec::new() });
                        }
                    None
                }
            };
            match pending {
                Some(p) if p.contains(';') => {
                    if let (Some(m), Some(c)) = (parse_decl(&p), classes.last_mut()) {
                        c.methods.push(m);
                    }
                    (classes, None)
                }
                p => (classes, p),
            }
        },
    );
    classes.retain(|c| !c.methods.is_empty());
    classes
}

/// MSVC: overloads are grouped at the first declaration, in reverse declaration order.
pub fn msvc_order(methods: &[Method]) -> Vec<Method> {
    let firsts: Vec<&str> = methods.iter().map(|m| m.name.as_str()).fold(Vec::new(), |mut v, n| {
        if !v.contains(&n) {
            v.push(n);
        }
        v
    });
    firsts
        .iter()
        .flat_map(|n| methods.iter().filter(move |m| m.name == *n).rev().cloned().collect::<Vec<_>>())
        .collect()
}

fn rust_type(t: &str) -> Option<&'static str> {
    Some(match t {
        "void" | "" => "()",
        "int" => "c_int",
        "bool" => "bool",
        "double" => "f64",
        "float" => "f32",
        "long" => "c_long",
        "size_t" => "usize",
        "unsigned int" => "c_uint",
        "unsigned char" => "u8",
        "unsigned int*" => "*mut c_uint",
        "const char*" => "*const c_char",
        "char*" => "*mut c_char",
        "const char**" => "*mut *const c_char",
        "CFunc" => "CFunc",
        "lua_State*" => "*mut lua_State",
        "const QAngle&" | "const Vector&" => "*const [f32; 3]",
        "fnptr" => "*const c_void",
        t if t.contains("std::") || t.contains("Bootil") => return None,
        t if t.ends_with('*') || t.ends_with('&') => "*mut c_void",
        _ => return None, // C++ object by value
    })
}

fn snake(n: &str) -> String {
    n.chars().enumerate().fold(String::new(), |mut s, (i, c)| {
        let prev_lower = i > 0 && n[..i].chars().last().is_some_and(|p| p.is_lowercase() || p.is_ascii_digit());
        if c.is_uppercase() && prev_lower {
            s.push('_');
        }
        s.extend(c.to_lowercase());
        s
    })
}

/// Unique field names: overloads get `_1`, `_2`, ... in declaration order.
fn field_names(methods: &[Method]) -> Vec<(Method, String)> {
    methods
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let count = methods.iter().filter(|o| o.name == m.name).count();
            let nth = methods[..i].iter().filter(|o| o.name == m.name).count();
            let base = match (m.destructor, m.name.as_str()) {
                (true, _) => "destructor".to_owned(),
                (_, "Type") => "type_".to_owned(),
                _ => snake(&m.name),
            };
            let name = if count > 1 { format!("{base}_{}", nth + 1) } else { base };
            (m.clone(), name)
        })
        .collect()
}

fn field(m: &Method, name: &str) -> String {
    let params: Option<Vec<&str>> = m.params.iter().map(|p| rust_type(p)).collect();
    let ret = rust_type(&m.ret);
    match (params, ret, FORCED_PADDING.contains(&m.name.as_str())) {
        (Some(ps), Some(r), false) => {
            let args = std::iter::once("*mut c_void").chain(ps).collect::<Vec<_>>().join(", ");
            let ret = if r == "()" { String::new() } else { format!(" -> {r}") };
            match m.variadic {
                true => format!("    pub {name}: unsafe extern \"C\" fn({args}, ...){ret},"),
                false => format!("    pub {name}: vfn!(({args}){ret}),"),
            }
        }
        _ => format!("    /// padding: `{}` (not callable from Rust)\n    pub {name}: Pad,", m.name),
    }
}

/// Slot order of one class's own virtuals for the target ABI.
fn own_slots(class: &Class, windows: bool) -> Vec<Method> {
    let ordered = match windows {
        true => msvc_order(&class.methods),
        false => class.methods.clone(),
    };
    // Itanium emits two destructor slots (complete + deleting), MSVC one.
    ordered
        .into_iter()
        .flat_map(|m| match (m.destructor, windows) {
            (true, false) => vec![m.clone(), m],
            _ => vec![m],
        })
        .collect()
}

/// Rust source for one class layout (base slots first).
fn emit_struct(class: &Class, base: Option<&Class>, windows: bool) -> String {
    let slots: Vec<Method> = base
        .map(|b| own_slots(b, windows))
        .unwrap_or_default()
        .into_iter()
        .chain(own_slots(class, windows))
        .collect();
    let fields = field_names(&slots).iter().map(|(m, n)| field(m, n)).collect::<Vec<_>>().join("\n");
    format!("/// {} slots\n#[repr(C)]\npub struct {}VTable {{\n{fields}\n}}\n", slots.len(), class.name)
}

pub fn generate(dir: &Path) -> Result<String, String> {
    let classes: Vec<Class> = HEADERS
        .iter()
        .map(|h| std::fs::read_to_string(dir.join(h)).map_err(|e| format!("{h}: {e}")))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .flat_map(|s| parse_header(s))
        .collect();
    let find = |n: &str| classes.iter().find(|c| c.name == n).ok_or_else(|| format!("class {n} not found"));
    let emit = |windows: bool| -> Result<String, String> {
        CLASSES
            .iter()
            .map(|(n, b)| Ok(emit_struct(find(n)?, b.map(find).transpose()?, windows)))
            .collect::<Result<Vec<_>, String>>()
            .map(|v| v.join("\n"))
    };
    Ok(format!(
        "// @generated by `cargo xtask gen-vtables` from {REPO} @ {COMMIT}. Do not edit.\n\
         #![allow(missing_docs)]\n\n\
         use core::ffi::{{c_char, c_int, c_long, c_uint, c_void}};\n\
         use crate::lua::{{CFunc, lua_State}};\n\
         use crate::vfn;\n\n\
         /// Slot that must not be called (C++ by-value types or raising members).\n\
         pub type Pad = usize;\n\n\
         #[cfg(not(windows))]\nmod layout {{\n    use super::*;\n{}\n}}\n\n\
         #[cfg(windows)]\nmod layout {{\n    use super::*;\n{}\n}}\n\n\
         pub use layout::*;\n",
        indent(&emit(false)?),
        indent(&emit(true)?)
    ))
}

fn indent(s: &str) -> String {
    s.lines().map(|l| if l.is_empty() { String::new() } else { format!("    {l}") }).collect::<Vec<_>>().join("\n")
}

/// Shallow-fetches the pinned commit into `target/garrysmod_common`.
pub fn fetch(root: &Path) -> Result<PathBuf, String> {
    let dir = root.join("target/garrysmod_common");
    let git = |args: &[&str]| -> Result<(), String> {
        Command::new("git").args(args).current_dir(&dir).status()
            .map_err(|e| e.to_string())?
            .success()
            .then_some(())
            .ok_or_else(|| format!("git {args:?} failed"))
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !dir.join(".git").exists() {
        git(&["init", "-q"])?;
        git(&["remote", "add", "origin", REPO])?;
    }
    git(&["fetch", "-q", "--depth", "1", "origin", COMMIT])
        .or_else(|_| git(&["fetch", "-q", "origin"]))?;
    git(&["checkout", "-q", COMMIT])?;
    Ok(dir.join("include/GarrysMod/Lua"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decl() {
        let m = parse_decl("virtual const char* GetString( int iStackPos = -1, unsigned int* iOutLen = nullptr ) = 0;").unwrap();
        assert_eq!((m.name.as_str(), m.ret.as_str()), ("GetString", "const char*"));
        assert_eq!(m.params, vec!["int", "unsigned int*"]);
        let v = parse_decl("virtual void Msg( const char *fmt, ... ) = 0;").unwrap();
        assert!(v.variadic);
        assert_eq!(parse_decl("virtual void SetType( unsigned char ) = 0;").unwrap().params, vec!["unsigned char"]);
        assert!(parse_decl("virtual ~ILuaShared( ) = 0;").unwrap().destructor);
    }

    #[test]
    fn msvc_groups_overloads_reversed() {
        let m = |n: &str, p: &str| Method { name: n.into(), ret: "void".into(), params: vec![p.into()], variadic: false, destructor: false };
        let order = msvc_order(&[m("A", "1"), m("S", "1"), m("B", "1"), m("S", "2")]);
        let got: Vec<_> = order.iter().map(|x| format!("{}{}", x.name, x.params[0])).collect();
        assert_eq!(got, ["A1", "S2", "S1", "B1"]);
    }
}
