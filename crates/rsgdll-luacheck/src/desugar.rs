//! GLua -> Lua 5.1 desugaring (spec §8.3 fallback, applied on every target): `!=`, `!`,
//! `&&`, `||`, `//` / `/* */` comments, `continue` -> `goto __continue_N`. Inter-token text
//! is kept (comments become their newlines) so Lua line numbers match the source.

use crate::lexer::{Tok, Token, lex};
use crate::{Diagnostic, Kind, Severity};

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Func,
    If,
    Loop(usize),
    Do,
    Repeat(usize),
}

/// Original text of a token, or its Lua 5.1 replacement.
fn text<'a>(src: &'a str, t: &Token) -> std::borrow::Cow<'a, str> {
    let raw = &src[t.span.start..t.span.end];
    match (&t.tok, raw) {
        (Tok::Sym("~="), "!=") => "~=".into(),
        (Tok::Sym("not"), "!") => "not ".into(),
        (Tok::Sym("and"), "&&") => " and ".into(),
        (Tok::Sym("or"), "||") => " or ".into(),
        _ => raw.into(),
    }
}

/// Gap between tokens with comments reduced to their newlines.
fn gap(src: &str, from: usize, to: usize) -> String {
    let g = &src[from..to];
    match g.trim().is_empty() {
        true => g.to_owned(),
        false => {
            let nl = g.matches('\n').count();
            if nl == 0 {
                " ".to_owned()
            } else {
                "\n".repeat(nl)
            }
        }
    }
}

pub fn desugar(src: &str) -> Result<String, Diagnostic> {
    let toks = lex(src).map_err(|e| Diagnostic {
        severity: Severity::Error,
        kind: Kind::Syntax,
        msg: e.msg,
        span: e.span,
    })?;
    let mut out = String::new();
    let mut stack: Vec<Block> = Vec::new();
    let mut loop_do = false;
    let mut next_label = 0usize;
    let mut used: Vec<usize> = Vec::new();
    let mut at = 0usize;
    for t in toks.iter().filter(|t| t.tok != Tok::Eof) {
        out.push_str(&gap(src, at, t.span.start));
        at = t.span.end;
        let label = |id: usize, used: &Vec<usize>| {
            if used.contains(&id) {
                format!(" ::__continue_{id}:: ")
            } else {
                Default::default()
            }
        };
        match &t.tok {
            Tok::Sym("function") => stack.push(Block::Func),
            Tok::Sym("if") => stack.push(Block::If),
            Tok::Sym("while" | "for") => {
                next_label += 1;
                stack.push(Block::Loop(next_label));
                loop_do = true;
            }
            Tok::Sym("do") if loop_do => loop_do = false,
            Tok::Sym("do") => stack.push(Block::Do),
            Tok::Sym("repeat") => {
                next_label += 1;
                stack.push(Block::Repeat(next_label));
            }
            Tok::Sym("end") => {
                if let Some(Block::Loop(id)) = stack.pop() {
                    out.push_str(&label(id, &used));
                }
            }
            Tok::Sym("until") => {
                if let Some(Block::Repeat(id)) = stack.pop() {
                    out.push_str(&label(id, &used));
                }
            }
            Tok::Sym("continue") => {
                let target = stack
                    .iter()
                    .rev()
                    .take_while(|b| **b != Block::Func)
                    .find_map(|b| match b {
                        Block::Loop(id) | Block::Repeat(id) => Some(*id),
                        _ => None,
                    });
                match target {
                    Some(id) => {
                        used.push(id);
                        out.push_str(&format!("goto __continue_{id}"));
                        continue;
                    }
                    None => {
                        return Err(Diagnostic {
                            severity: Severity::Error,
                            kind: Kind::Syntax,
                            msg: "'continue' outside a loop".into(),
                            span: t.span,
                        });
                    }
                }
            }
            _ => {}
        }
        out.push_str(&text(src, t));
    }
    out.push_str(&gap(src, at, src.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_glua() {
        assert_eq!(
            desugar("if a != b && !c || d then end // x").unwrap(),
            "if a ~= b  and  not c  or  d then end "
        );
        assert_eq!(
            desugar("for i=1,3 do if i==2 then continue end print(i) end").unwrap(),
            "for i=1,3 do if i==2 then goto __continue_1 end print(i)  ::__continue_1:: end"
        );
        assert_eq!(desugar("a = 1 /* x\ny */ b = 2").unwrap(), "a = 1\nb = 2");
        assert!(desugar("continue").is_err());
        // nested function resets loop context
        assert!(desugar("while x do local f = function() continue end end").is_err());
    }
}
