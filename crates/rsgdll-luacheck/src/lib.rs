//! GLua front-end and compile-time checker for `lua!` (spec §9). No FFI dependency, so it
//! runs inside the proc macro.

pub mod ast;
pub mod desugar;
pub mod lexer;
pub mod parser;

use std::collections::HashSet;

use ast::{Block, Expr, FuncBody, Stat};
use rsgdll_apidb::{ApiDb, Realm};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Which check produced a diagnostic (`unchecked` silences member/arity/realm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Syntax,
    UnknownGlobal,
    UnknownMember,
    Arity,
    /// Realm violation; `realm` is the function's realm. Emitted cfg-gated by the macro.
    Realm(Realm),
    Deprecated,
    ReturnCount,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: Kind,
    pub msg: String,
    pub span: Span,
}

/// Inputs from the call site.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    /// Capture names with optional known Lua class (`ply` -> `Player`).
    pub captures: Vec<(String, Option<String>)>,
    /// Extra allowed globals (`#[rsgdll::lua_globals]` / crate allowlist).
    pub globals: Vec<String>,
    /// Expected return count of the Rust binding (tuple of N), if known.
    pub expected_returns: Option<usize>,
}

/// Levenshtein distance for "did you mean".
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    a.chars()
        .fold((0..=b.len()).collect::<Vec<_>>(), |prev, ca| {
            let mut row = vec![prev[0] + 1];
            b.iter().enumerate().for_each(|(j, cb)| {
                let v = (prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(row[j] + 1);
                row.push(v);
            });
            row
        })
        .last()
        .copied()
        .unwrap_or(0)
}

/// Closest candidate within a small edit distance.
pub fn suggest<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let lname = name.to_lowercase();
    candidates
        .map(|c| (distance(&lname, &c.to_lowercase()), c))
        .filter(|(d, _)| *d <= (name.len() / 3).max(2))
        .min_by_key(|(d, c)| (*d, c.len()))
        .map(|(_, c)| c)
}

/// Lua 5.1 / LuaJIT base names the snippets may not list.
const BUILTINS: &[&str] = &[
    "_G", "_VERSION", "assert", "collectgarbage", "error", "getfenv", "getmetatable", "ipairs", "load",
    "loadstring", "module", "next", "pairs", "pcall", "print", "rawequal", "rawget", "rawset", "require",
    "select", "setfenv", "setmetatable", "tonumber", "tostring", "type", "unpack", "xpcall", "self",
    "coroutine", "debug", "io", "math", "os", "package", "string", "table", "bit", "jit", "ffi",
];

struct Checker<'a> {
    db: &'a ApiDb,
    ctx: &'a Ctx,
    scopes: Vec<HashSet<String>>,
    out: Vec<Diagnostic>,
    returns: Vec<(usize, bool, Span)>,
}

impl<'a> Checker<'a> {
    fn push(&mut self, severity: Severity, kind: Kind, msg: String, span: Span) {
        self.out.push(Diagnostic { severity, kind, msg, span });
    }

    fn is_local(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(n))
    }

    fn declare(&mut self, names: impl IntoIterator<Item = String>) {
        if let Some(s) = self.scopes.last_mut() {
            s.extend(names);
        }
    }

    fn scoped(&mut self, names: Vec<String>, f: impl FnOnce(&mut Self)) {
        self.scopes.push(names.into_iter().collect());
        f(self);
        self.scopes.pop();
    }

    fn known_global(&self, n: &str) -> bool {
        BUILTINS.contains(&n) || self.db.is_global(n) || self.ctx.globals.iter().any(|g| g == n)
    }

    /// Lua class of an expression when knowable (captures, known-return calls).
    fn class_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Capture(c, _) => self.ctx.captures.iter().find(|(n, _)| n == c).and_then(|(_, t)| t.clone()),
            Expr::Call(f, _, _) => f.path().and_then(|p| self.db.functions.get(&p)).and_then(|f| f.returns.first().cloned()),
            Expr::Method(r, m, _, _) => self.class_of(r).and_then(|c| self.db.method(&c, m)).and_then(|f| f.returns.first().cloned()),
            Expr::Paren(e) => self.class_of(e),
            _ => None,
        }
        .filter(|c| self.db.classes.contains_key(c))
    }

    fn arity(&mut self, f: &rsgdll_apidb::Function, args: &[Expr], span: Span, extra: usize) {
        let open_ended = args.last().is_some_and(Expr::is_multi);
        let n = args.len() + extra;
        let too_few = n < f.min_args && !open_ended;
        let too_many = f.max_args.is_some_and(|m| n > m);
        if too_few || too_many {
            let want = match f.max_args {
                Some(m) if m == f.min_args => m.to_string(),
                Some(m) => format!("{}..={m}", f.min_args),
                None => format!("at least {}", f.min_args),
            };
            self.push(Severity::Warning, Kind::Arity, format!("'{}' expects {want} argument(s), got {n}", f.path), span);
        }
    }

    fn func_checks(&mut self, f: &rsgdll_apidb::Function, span: Span) {
        if f.deprecated {
            self.push(Severity::Warning, Kind::Deprecated, format!("'{}' is deprecated", f.path), span);
        }
        if matches!(f.realm, Realm::Server | Realm::Client | Realm::Menu) {
            self.push(Severity::Error, Kind::Realm(f.realm), format!("'{}' is {}-only", f.path, f.realm.as_str()), span);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Name(n, sp) => {
                if !self.is_local(n) && !self.known_global(n) {
                    let hint = suggest(n, self.db.libraries.keys().chain(self.db.functions.keys()).map(String::as_str).filter(|k| !k.contains(['.', ':'])))
                        .map_or(String::new(), |s| format!(" (did you mean '{s}'?)"));
                    self.push(Severity::Warning, Kind::UnknownGlobal, format!("unknown global '{n}'{hint}"), *sp);
                }
            }
            Expr::Field(base, field, sp) => {
                self.expr(base);
                if let Some(Expr::Name(lib, _)) = Some(&**base).filter(|_| true) {
                    if !self.is_local(lib) && self.db.libraries.contains_key(lib) && !self.db.functions.contains_key(&format!("{lib}.{field}")) && !self.db.variables.contains_key(&format!("{lib}.{field}")) {
                        let members: Vec<&str> = self.db.members(lib).collect();
                        // Libraries the snippets list without members (e.g. pure Lua std) are not checked.
                        if !members.is_empty() {
                            let hint = suggest(field, members.into_iter()).map_or(String::new(), |s| format!(" (did you mean '{lib}.{s}'?)"));
                            self.push(Severity::Error, Kind::UnknownMember, format!("unknown member '{lib}.{field}'{hint}"), *sp);
                        }
                    }
                }
            }
            Expr::Index(b, k, _) => {
                self.expr(b);
                self.expr(k);
            }
            Expr::Call(f, args, sp) => {
                self.expr(f);
                args.iter().for_each(|a| self.expr(a));
                let local_root = matches!(&**f, Expr::Name(n, _) if self.is_local(n))
                    || matches!(&**f, Expr::Field(b, _, _) if matches!(&**b, Expr::Name(n, _) if self.is_local(n)));
                if let Some(func) = f.path().filter(|_| !local_root).and_then(|p| self.db.functions.get(&p)).cloned() {
                    self.arity(&func, args, *sp, 0);
                    self.func_checks(&func, *sp);
                }
            }
            Expr::Method(recv, m, args, sp) => {
                self.expr(recv);
                args.iter().for_each(|a| self.expr(a));
                if let Some(class) = self.class_of(recv) {
                    match self.db.method(&class, m).cloned() {
                        Some(func) => {
                            self.arity(&func, args, *sp, 0);
                            self.func_checks(&func, *sp);
                        }
                        None => {
                            let methods = self.db.methods(&class);
                            let hint = suggest(m, methods.iter().map(String::as_str)).map_or(String::new(), |s| format!(" (did you mean '{class}:{s}'?)"));
                            self.push(Severity::Error, Kind::UnknownMember, format!("unknown method '{class}:{m}'{hint}"), *sp);
                        }
                    }
                }
            }
            Expr::Function(body) => self.funcbody(body),
            Expr::Table(items) => items.iter().for_each(|(k, v)| {
                if let Some(k) = k {
                    self.expr(k);
                }
                self.expr(v);
            }),
            Expr::Bin(a, _, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Un(_, e) | Expr::Paren(e) => self.expr(e),
            Expr::Nil | Expr::True | Expr::False | Expr::Vararg | Expr::Number(_) | Expr::Str(_) | Expr::Capture(..) => {}
        }
    }

    fn funcbody(&mut self, f: &FuncBody) {
        // Returns inside nested functions are not chunk returns.
        let saved = std::mem::take(&mut self.returns);
        self.scoped(f.params.clone(), |c| c.block(&f.body));
        self.returns = saved;
    }

    fn block(&mut self, b: &Block) {
        self.scoped(Vec::new(), |c| b.iter().for_each(|s| c.stat(s)));
    }

    fn stat(&mut self, s: &Stat) {
        match s {
            Stat::Expr(e) => self.expr(e),
            Stat::Assign(targets, vals) => {
                vals.iter().for_each(|v| self.expr(v));
                targets.iter().for_each(|t| match t {
                    // Assigning a global defines it; do not warn.
                    Expr::Name(..) => {}
                    t => self.expr(t),
                });
            }
            Stat::Local(names, vals) => {
                vals.iter().for_each(|v| self.expr(v));
                self.declare(names.clone());
            }
            Stat::LocalFunction(n, body) => {
                self.declare([n.clone()]);
                self.funcbody(body);
            }
            Stat::Function(target, body) => {
                if let Some(Expr::Name(..)) = Some(target) {
                } else if let Expr::Field(b, _, _) = target {
                    self.expr(b);
                }
                self.funcbody(body);
            }
            Stat::Do(b) => self.block(b),
            Stat::While(c, b) => {
                self.expr(c);
                self.block(b);
            }
            Stat::Repeat(b, c) => self.scoped(Vec::new(), |ch| {
                b.iter().for_each(|s| ch.stat(s));
                ch.expr(c);
            }),
            Stat::If(arms, els) => {
                arms.iter().for_each(|(c, b)| {
                    self.expr(c);
                    self.block(b);
                });
                if let Some(b) = els {
                    self.block(b);
                }
            }
            Stat::NumFor(v, a, b, step, body) => {
                [Some(a), Some(b), step.as_ref()].into_iter().flatten().for_each(|e| self.expr(e));
                self.scoped(vec![v.clone()], |c| c.block(body));
            }
            Stat::GenFor(names, exprs, body) => {
                exprs.iter().for_each(|e| self.expr(e));
                self.scoped(names.clone(), |c| c.block(body));
            }
            Stat::Return(vals, sp) => {
                vals.iter().for_each(|v| self.expr(v));
                self.returns.push((vals.len(), vals.last().is_some_and(Expr::is_multi), *sp));
            }
            Stat::Break | Stat::Continue | Stat::Goto(_) | Stat::Label(_) => {}
        }
    }
}

/// Parses and checks a chunk. Syntax errors return a single `Kind::Syntax` diagnostic.
pub fn check(src: &str, db: &ApiDb, ctx: &Ctx) -> Vec<Diagnostic> {
    let parsed = lexer::lex(src)
        .map_err(|e| (e.msg, e.span))
        .and_then(|t| parser::parse(&t).map_err(|e| (e.msg, e.span)));
    match parsed {
        Err((msg, span)) => vec![Diagnostic { severity: Severity::Error, kind: Kind::Syntax, msg, span }],
        Ok(block) => {
            let captures = ctx.captures.iter().map(|(n, _)| n.clone()).collect();
            let mut c = Checker { db, ctx, scopes: vec![captures], out: Vec::new(), returns: Vec::new() };
            c.block(&block);
            if let Some(want) = ctx.expected_returns.filter(|w| *w > 1) {
                let fixed: Vec<_> = c.returns.iter().filter(|(_, multi, _)| !multi).collect();
                if !c.returns.is_empty() && fixed.len() == c.returns.len() && fixed.iter().all(|(n, _, _)| *n != want) {
                    let span = fixed.first().map_or_else(Span::default, |r| r.2);
                    c.push(Severity::Warning, Kind::ReturnCount, format!("chunk never returns {want} values"), span);
                }
            }
            c.out
        }
    }
}

/// Capture names in source order (first occurrence), for the `local __a1, ... = ...` rewrite.
pub fn captures(src: &str) -> Result<Vec<(String, Span)>, Diagnostic> {
    lexer::lex(src)
        .map(|toks| {
            toks.into_iter()
                .filter_map(|t| match t.tok {
                    lexer::Tok::Capture(c) => Some((c, t.span)),
                    _ => None,
                })
                .collect()
        })
        .map_err(|e| Diagnostic { severity: Severity::Error, kind: Kind::Syntax, msg: e.msg, span: e.span })
}

/// Rewrites `$name` captures to `__rsgdll_<name>` locals bound from `...` (spec §8.3).
/// Returns the new source and the ordered unique capture names.
pub fn rewrite(src: &str) -> Result<(String, Vec<String>), Diagnostic> {
    let caps = captures(src)?;
    let names: Vec<String> = caps.iter().fold(Vec::new(), |mut v, (n, _)| {
        if !v.contains(n) {
            v.push(n.clone());
        }
        v
    });
    let (body, last) = caps.iter().fold((String::new(), 0), |(mut s, at), (n, sp)| {
        s.push_str(&src[at..sp.start]);
        s.push_str("__rsgdll_");
        s.push_str(n);
        (s, sp.end)
    });
    let body = body + &src[last..];
    let header = match names.is_empty() {
        true => String::new(),
        // Same line as the body so Lua line numbers match the source.
        false => format!("local {} = ... ", names.iter().map(|n| format!("__rsgdll_{n}")).collect::<Vec<_>>().join(", ")),
    };
    Ok((header + &body, names))
}

/// Whether the whole chunk is one expression list (`GetHostName()`, `#player.GetAll()`),
/// so `lua!` can return its value like an expression.
pub fn is_expression(src: &str) -> bool {
    let wrapped = format!("return {src}");
    lexer::lex(&wrapped).is_ok_and(|t| parser::parse(&t).is_ok())
}

/// Byte offset -> (line, column), both 0-based.
pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count();
    let col = before.rfind('\n').map_or(before.len(), |p| before.len() - p - 1);
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diags(src: &str, ctx: &Ctx) -> Vec<(Kind, String)> {
        check(src, rsgdll_apidb::db(), ctx).into_iter().map(|d| (d.kind, d.msg)).collect()
    }

    #[test]
    fn spec_example_is_clean() {
        let ctx = Ctx { captures: vec![("ply".into(), Some("Player".into())), ("limit".into(), None)], ..Ctx::default() };
        let src = "local n = 0\nfor _, e in ipairs(ents.FindByClass(\"prop_physics\")) do\n if e:GetOwner() == $ply then n = n + 1 end\nend\nreturn math.min(n, $limit)";
        let d: Vec<_> = diags(src, &ctx).into_iter().filter(|(k, _)| !matches!(k, Kind::Realm(_))).collect();
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn unknown_member_with_suggestion() {
        let d = diags("net.WriteStrng('x')", &Ctx::default());
        assert!(d.iter().any(|(k, m)| *k == Kind::UnknownMember && m.contains("did you mean 'net.WriteString'")), "{d:?}");
    }

    #[test]
    fn unknown_global_warns_unless_allowed() {
        assert!(diags("return MyAddonThing", &Ctx::default()).iter().any(|(k, _)| *k == Kind::UnknownGlobal));
        let ctx = Ctx { globals: vec!["MyAddonThing".into()], ..Ctx::default() };
        assert!(diags("return MyAddonThing", &ctx).is_empty());
        assert!(diags("local x = 1 return x", &Ctx::default()).is_empty());
    }

    #[test]
    fn arity_and_realm() {
        let d = diags("GetHostName(1, 2)", &Ctx::default());
        assert!(d.iter().any(|(k, _)| *k == Kind::Arity), "{d:?}");
        let d = diags("net.Broadcast()", &Ctx::default());
        assert!(d.iter().any(|(k, _)| *k == Kind::Realm(Realm::Server)), "{d:?}");
        assert!(diags("print(1, 2, 3, 4, 5)", &Ctx::default()).is_empty());
    }

    #[test]
    fn typed_method_calls() {
        let ctx = Ctx { captures: vec![("p".into(), Some("Player".into()))], ..Ctx::default() };
        let d = diags("return $p:Nikc()", &ctx);
        assert!(d.iter().any(|(k, m)| *k == Kind::UnknownMember && m.contains("Nick")), "{d:?}");
        // inherited from Entity
        assert!(diags("return $p:GetPos()", &ctx).iter().all(|(k, _)| !matches!(k, Kind::UnknownMember)));
    }

    #[test]
    fn return_count() {
        let ctx = Ctx { expected_returns: Some(2), ..Ctx::default() };
        assert!(diags("return 1", &ctx).iter().any(|(k, _)| *k == Kind::ReturnCount));
        assert!(diags("if x then return 1, 2 end return 1", &Ctx { globals: vec!["x".into()], ..ctx.clone() }).is_empty());
    }

    #[test]
    fn syntax_error_has_span() {
        let d = check("local x = = 1", rsgdll_apidb::db(), &Ctx::default());
        assert_eq!(d[0].kind, Kind::Syntax);
        assert_eq!(line_col("a\nbc", 3), (1, 1));
    }

    #[test]
    fn expression_chunks() {
        assert!(is_expression("GetHostName()"));
        assert!(is_expression("#player.GetAll(), $x"));
        assert!(!is_expression("local x = 1 return x"));
        assert!(!is_expression("a() b()"));
        assert!(!is_expression("return 1"));
    }

    #[test]
    fn rewrite_captures() {
        let (src, names) = rewrite("return $a + $b + $a").unwrap();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(src, "local __rsgdll_a, __rsgdll_b = ... return __rsgdll_a + __rsgdll_b + __rsgdll_a");
    }
}
