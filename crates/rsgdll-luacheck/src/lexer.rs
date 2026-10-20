//! GLua lexer: Lua 5.1 + LuaJIT + GLua (`!=`, `!`, `&&`, `||`, `//`, `/* */`, `continue`).
//! Every token keeps its byte span for error mapping (spec §9.2).

use crate::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Name(String),
    Number(f64),
    Str(String),
    /// `$ident` capture (rsgdll extension).
    Capture(String),
    /// Keywords and symbols (`and`, `~=`, `..`, `(`, ...). GLua aliases are normalised:
    /// `!=` -> `~=`, `!` -> `not`, `&&` -> `and`, `||` -> `or`.
    Sym(&'static str),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexError {
    pub msg: String,
    pub span: Span,
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "continue",
];

/// Longest symbols first.
const SYMBOLS: &[(&str, &str)] = &[
    ("...", "..."),
    ("==", "=="),
    ("~=", "~="),
    ("!=", "~="),
    ("<=", "<="),
    (">=", ">="),
    ("&&", "and"),
    ("||", "or"),
    ("..", ".."),
    ("::", "::"),
    ("+", "+"),
    ("-", "-"),
    ("*", "*"),
    ("/", "/"),
    ("%", "%"),
    ("^", "^"),
    ("#", "#"),
    ("<", "<"),
    (">", ">"),
    ("=", "="),
    ("(", "("),
    (")", ")"),
    ("{", "{"),
    ("}", "}"),
    ("[", "["),
    ("]", "]"),
    (";", ";"),
    (":", ":"),
    (",", ","),
    (".", "."),
    ("!", "not"),
];

fn keyword(s: &str) -> Option<&'static str> {
    KEYWORDS.iter().copied().find(|k| *k == s)
}

/// `[[`/`[==[` opener level at `i`, if any.
fn long_open(b: &[u8], i: usize) -> Option<usize> {
    let eqs = b.get(i + 1..)?.iter().take_while(|c| **c == b'=').count();
    (b.get(i) == Some(&b'[') && b.get(i + 1 + eqs) == Some(&b'[')).then_some(eqs)
}

/// Index just past the matching `]==]`.
fn long_close(src: &str, from: usize, eqs: usize) -> Option<usize> {
    let close = format!("]{}]", "=".repeat(eqs));
    src.get(from..)?
        .find(&close)
        .map(|p| from + p + close.len())
}

fn escape(src: &str, i: usize) -> Result<(String, usize), String> {
    let b = src.as_bytes();
    let c = *b.get(i).ok_or("unfinished escape")?;
    Ok(match c {
        b'n' => ("\n".into(), i + 1),
        b't' => ("\t".into(), i + 1),
        b'r' => ("\r".into(), i + 1),
        b'a' => ("\x07".into(), i + 1),
        b'b' => ("\x08".into(), i + 1),
        b'f' => ("\x0c".into(), i + 1),
        b'v' => ("\x0b".into(), i + 1),
        b'\\' | b'"' | b'\'' | b'\n' => ((c as char).to_string(), i + 1),
        b'x' => {
            let h = src.get(i + 1..i + 3).ok_or("bad \\x escape")?;
            let v = u8::from_str_radix(h, 16).map_err(|_| "bad \\x escape")?;
            (char::from(v).to_string(), i + 3)
        }
        b'0'..=b'9' => {
            let n = b[i..]
                .iter()
                .take(3)
                .take_while(|c| c.is_ascii_digit())
                .count();
            let v: u32 = src[i..i + n].parse().map_err(|_| "bad escape")?;
            (
                char::from_u32(v)
                    .filter(|_| v < 256)
                    .ok_or("escape too large")?
                    .to_string(),
                i + n,
            )
        }
        b'z' => (
            "".into(),
            i + 1
                + b[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_whitespace())
                    .count(),
        ),
        _ => return Err(format!("invalid escape '\\{}'", c as char)),
    })
}

fn number(src: &str, i: usize) -> Result<(f64, usize), String> {
    let b = src.as_bytes();
    let hex = b.get(i) == Some(&b'0') && matches!(b.get(i + 1), Some(b'x' | b'X'));
    let end = (i + if hex { 2 } else { 0 }..b.len())
        .find(|&j| {
            let c = b[j];
            let prev = b.get(j.wrapping_sub(1)).copied().unwrap_or(0);
            let exp = if hex { b"pP" } else { b"eE" };
            !(c.is_ascii_alphanumeric()
                || c == b'.'
                || ((c == b'+' || c == b'-') && exp.contains(&prev)))
        })
        .unwrap_or(b.len());
    let text = src[i..end].trim_end_matches(['u', 'U', 'l', 'L', 'i', 'I']);
    let v = match hex {
        true => i64::from_str_radix(&text[2..], 16).map(|v| v as f64).ok(),
        false => text.parse::<f64>().ok(),
    };
    v.map(|v| (v, end))
        .ok_or_else(|| format!("malformed number near '{}'", &src[i..end]))
}

/// Tokenises `src`. Comments (`--`, `--[[ ]]`, `//`, `/* */`) are skipped.
pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let b = src.as_bytes();
    let err = |msg: String, s: usize, e: usize| LexError {
        msg,
        span: Span { start: s, end: e },
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        let push = |out: &mut Vec<Token>, tok: Tok, end: usize| {
            out.push(Token {
                tok,
                span: Span { start, end },
            })
        };
        match c {
            _ if c.is_ascii_whitespace() => i += 1,
            b'-' if b.get(i + 1) == Some(&b'-') => {
                i = match long_open(b, i + 2) {
                    Some(eqs) => long_close(src, i + 4 + eqs, eqs)
                        .ok_or_else(|| err("unfinished long comment".into(), start, b.len()))?,
                    None => src[i..].find('\n').map_or(b.len(), |p| i + p),
                };
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = src[i..].find('\n').map_or(b.len(), |p| i + p)
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..]
                    .find("*/")
                    .map(|p| i + 2 + p + 2)
                    .ok_or_else(|| err("unfinished /* comment".into(), start, b.len()))?;
            }
            b'[' if long_open(b, i).is_some() => {
                let eqs = long_open(b, i).unwrap_or(0);
                let body = i + 2 + eqs;
                let end = long_close(src, body, eqs)
                    .ok_or_else(|| err("unfinished long string".into(), start, b.len()))?;
                let text = &src[body..end - 2 - eqs];
                push(
                    &mut out,
                    Tok::Str(text.strip_prefix('\n').unwrap_or(text).to_owned()),
                    end,
                );
                i = end;
            }
            b'"' | b'\'' => {
                let mut s = String::new();
                let mut j = i + 1;
                loop {
                    match b.get(j) {
                        None | Some(b'\n') => {
                            return Err(err("unfinished string".into(), start, j));
                        }
                        Some(q) if *q == c => break,
                        Some(b'\\') => {
                            let (e, next) = escape(src, j + 1).map_err(|m| err(m, j, j + 2))?;
                            s.push_str(&e);
                            j = next;
                        }
                        Some(_) => {
                            let ch = src[j..].chars().next().unwrap_or('\0');
                            s.push(ch);
                            j += ch.len_utf8();
                        }
                    }
                }
                push(&mut out, Tok::Str(s), j + 1);
                i = j + 1;
            }
            b'0'..=b'9' => {
                let (v, end) = number(src, i).map_err(|m| err(m, start, i + 1))?;
                push(&mut out, Tok::Number(v), end);
                i = end;
            }
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let (v, end) = number(src, i).map_err(|m| err(m, start, i + 1))?;
                push(&mut out, Tok::Number(v), end);
                i = end;
            }
            b'$' => {
                let end = (i + 1..b.len())
                    .find(|&j| !(b[j].is_ascii_alphanumeric() || b[j] == b'_'))
                    .unwrap_or(b.len());
                if end == i + 1 {
                    return Err(err("expected identifier after '$'".into(), start, end));
                }
                push(&mut out, Tok::Capture(src[i + 1..end].to_owned()), end);
                i = end;
            }
            _ if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                let end = (i..b.len())
                    .find(|&j| !(b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] >= 0x80))
                    .unwrap_or(b.len());
                let word = &src[i..end];
                push(
                    &mut out,
                    keyword(word).map_or_else(|| Tok::Name(word.to_owned()), Tok::Sym),
                    end,
                );
                i = end;
            }
            _ => match SYMBOLS.iter().find(|(s, _)| src[i..].starts_with(s)) {
                Some((s, norm)) => {
                    push(&mut out, Tok::Sym(norm), i + s.len());
                    i += s.len();
                }
                None => {
                    return Err(err(
                        format!(
                            "unexpected symbol '{}'",
                            src[i..].chars().next().unwrap_or('?')
                        ),
                        start,
                        i + 1,
                    ));
                }
            },
        }
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span {
            start: b.len(),
            end: b.len(),
        },
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn glua_aliases_and_comments() {
        assert_eq!(
            toks("a != !b && c || d // x\n/* y */ -- z\n--[[ w ]] continue"),
            [
                Tok::Name("a".into()),
                Tok::Sym("~="),
                Tok::Sym("not"),
                Tok::Name("b".into()),
                Tok::Sym("and"),
                Tok::Name("c".into()),
                Tok::Sym("or"),
                Tok::Name("d".into()),
                Tok::Sym("continue"),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn strings_numbers_captures() {
        assert_eq!(
            toks(r#""a\n" 'b' [[c]] 0x10 1e3 .5 $ply"#),
            [
                Tok::Str("a\n".into()),
                Tok::Str("b".into()),
                Tok::Str("c".into()),
                Tok::Number(16.0),
                Tok::Number(1000.0),
                Tok::Number(0.5),
                Tok::Capture("ply".into()),
                Tok::Eof
            ]
        );
        assert!(lex("'open").is_err());
        assert_eq!(lex("x = 1").unwrap()[1].span, Span { start: 2, end: 3 });
    }
}
