//! `gen-apidb`: parse glua-api-snippets (`library/*.lua`) into the vendored
//! `crates/rsgdll-apidb/src/data.txt` (format documented in rsgdll-apidb, spec §9.1).

use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = "https://github.com/luttje/glua-api-snippets";
pub const BRANCH: &str = "lua-language-server-addon";
/// Pinned snippets commit. Bump deliberately, then re-run and review the diff.
pub const COMMIT: &str = "b0150ff772839a0365cdd986b8e788cdc20f1a89";

#[derive(Default)]
struct Doc {
    realm: Option<&'static str>,
    deprecated: bool,
    params: Vec<(String, String, bool)>,
    returns: Vec<String>,
    ty: Option<String>,
}

/// Realm from the wiki badge (`![(Server)](...)`, `(Client and Menu)`, ...).
pub fn realm(line: &str) -> Option<&'static str> {
    let badge = line.split("![(").nth(1)?.split(")]").next()?;
    let has = |r: &str| badge.contains(r);
    Some(match (has("Server"), has("Client"), has("Shared"), has("Menu")) {
        (_, _, true, _) | (true, true, _, _) => "shared",
        (true, false, _, _) => "server",
        (false, true, _, _) => "client",
        (false, false, _, true) => "menu",
        _ => return None,
    })
}

/// First type token (stops at whitespace outside `<>`/`()`).
fn type_token(s: &str) -> String {
    let (out, _) = s.trim().chars().fold((String::new(), 0i32), |(mut out, depth), c| match (c, depth) {
        (_, d) if d < 0 => (out, d),
        (c, 0) if c.is_whitespace() => (out, -1),
        ('<' | '(', d) => { out.push(c); (out, d + 1) }
        ('>' | ')', d) => { out.push(c); (out, d - 1) }
        (c, d) => { out.push(c); (out, d) }
    });
    out.replace(['\t', ','], "")
}

fn doc_line(mut doc: Doc, line: &str) -> Doc {
    let body = line.trim_start_matches('-').trim();
    if let Some(r) = realm(line) {
        doc.realm.get_or_insert(r);
    }
    if body.starts_with("@deprecated") {
        doc.deprecated = true;
    } else if let Some(rest) = body.strip_prefix("@param ") {
        let (name, ty) = rest.split_once(char::is_whitespace).unwrap_or((rest, "any"));
        let optional = name.ends_with('?');
        doc.params.push((name.trim_end_matches('?').to_owned(), type_token(ty), optional));
    } else if let Some(rest) = body.strip_prefix("@return ") {
        doc.returns.push(type_token(rest));
    } else if let Some(rest) = body.strip_prefix("@type ") {
        doc.ty = Some(type_token(rest));
    }
    doc
}

fn params_field(params: &[(String, String, bool)]) -> String {
    match params.is_empty() {
        true => "-".into(),
        false => params
            .iter()
            .map(|(n, t, o)| format!("{n}:{t}{}", if *o { "?" } else { "" }))
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn list_field(v: &[String]) -> String {
    if v.is_empty() { "-".into() } else { v.join(",") }
}

/// One `function X(...) end` declaration with its doc block -> data line.
fn function_line(path: &str, doc: &Doc) -> String {
    let realm = doc.realm.unwrap_or("shared");
    match path.split_once(':').filter(|(o, _)| *o == "GM" || *o == "SANDBOX") {
        Some((owner, name)) => format!("H\t{owner}\t{name}\t{realm}\t{}\t{}", params_field(&doc.params), list_field(&doc.returns)),
        None => {
            let vararg = doc.params.iter().any(|(n, _, _)| n == "...");
            let fixed: Vec<_> = doc.params.iter().filter(|(n, _, _)| n != "...").cloned().collect();
            let min = fixed.iter().rposition(|(_, _, o)| !o).map_or(0, |i| i + 1);
            let max = if vararg { "-1".to_owned() } else { fixed.len().to_string() };
            format!(
                "F\t{path}\t{realm}\t{}\t{min}\t{max}\t{}\t{}",
                u8::from(doc.deprecated),
                params_field(&doc.params),
                list_field(&doc.returns)
            )
        }
    }
}

fn is_ident_path(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':'))
}

/// All data lines of one snippet file.
pub fn parse_file(src: &str) -> Vec<String> {
    let (lines, _) = src.lines().fold((Vec::new(), Doc::default()), |(mut out, doc), line| {
        let t = line.trim();
        if t.starts_with("---") {
            return (out, doc_line(doc, t));
        }
        if let Some(rest) = t.strip_prefix("function ") {
            let path = rest.split('(').next().unwrap_or("").trim();
            // Globals are declared as `function _G.Name`.
            let path = path.strip_prefix("_G.").unwrap_or(path);
            if is_ident_path(path) {
                out.push(function_line(path, &doc));
            }
        } else if let Some((name, value)) = t.split_once(" = ").filter(|(n, _)| is_ident_path(n) && !n.contains(['.', ':'])) {
            match value.trim() {
                "{}" => out.push(format!("L\t{name}")),
                "nil" => out.push(format!("V\t{name}\t{}", doc.ty.clone().unwrap_or_else(|| "any".into()))),
                v if v.parse::<f64>().is_ok() || v.starts_with('-') => out.push(format!("V\t{name}\tnumber")),
                v if v.starts_with('"') => out.push(format!("V\t{name}\tstring")),
                _ => out.push(format!("V\t{name}\tany")),
            }
        }
        (out, Doc::default())
    });
    // `---@class` lines are doc comments; collect them separately.
    lines
        .into_iter()
        .chain(src.lines().filter_map(|l| {
            let rest = l.trim().strip_prefix("---@class ")?;
            let (name, parent) = rest.split_once(':').map_or((rest.trim(), "-"), |(n, p)| (n.trim(), p.trim()));
            let parent = parent.split([',', ' ']).next().unwrap_or("-");
            Some(format!("C\t{name}\t{}", if parent.is_empty() { "-" } else { parent }))
        }))
        .collect()
}

pub fn generate(library: &Path) -> Result<String, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(library)
        .map_err(|e| format!("{}: {e}", library.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "lua"))
        .collect();
    files.sort();
    let mut lines: Vec<String> = files
        .iter()
        .map(|f| std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display())))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .flat_map(|s| parse_file(s))
        .collect();
    lines.sort();
    lines.dedup();
    Ok(lines.join("\n") + "\n")
}

/// Shallow-fetches the pinned commit into `target/glua-api-snippets`.
pub fn fetch(root: &Path) -> Result<PathBuf, String> {
    let dir = root.join("target/glua-api-snippets");
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
    git(&["fetch", "-q", "--depth", "1", "origin", COMMIT]).or_else(|_| git(&["fetch", "-q", "origin", BRANCH]))?;
    git(&["checkout", "-q", COMMIT])?;
    Ok(dir.join("library"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snippet() {
        let src = "net = {}\n---![(Shared)](x) Writes.\n---@param str string The string.\n---@param n? number Count.\nfunction net.WriteString(str, n) end\n\
                   ---![(Server)](x) Says.\n---@param sender Player The player.\n---@return string # text\nfunction GM:PlayerSay(sender) end\n\
                   ---@class Player : Entity\n---@type boolean\nSERVER = nil\nHUD_PRINTTALK = 3\n\
                   ---![(Shared)](x)\n---@param ... any Args.\nfunction _G.print(...) end\n";
        let lines = parse_file(src);
        assert!(lines.contains(&"L\tnet".to_owned()));
        assert!(lines.contains(&"F\tnet.WriteString\tshared\t0\t1\t2\tstr:string,n:number?\t-".to_owned()));
        assert!(lines.contains(&"H\tGM\tPlayerSay\tserver\tsender:Player\tstring".to_owned()));
        assert!(lines.contains(&"C\tPlayer\tEntity".to_owned()));
        assert!(lines.contains(&"V\tSERVER\tboolean".to_owned()));
        assert!(lines.contains(&"V\tHUD_PRINTTALK\tnumber".to_owned()));
        assert!(lines.contains(&"F\tprint\tshared\t0\t0\t-1\t...:any\t-".to_owned()));
        assert_eq!(realm("---![(Client and Menu)](x)"), Some("client"));
    }
}
