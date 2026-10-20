//! Hand-written recursive-descent GLua parser (spec §9.2 check 1).

use crate::Span;
use crate::ast::*;
use crate::lexer::{Tok, Token};

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

struct P<'t> {
    toks: &'t [Token],
    i: usize,
}

type R<T> = Result<T, ParseError>;

/// Binary operator precedences (left, right) from Lua 5.1 `lparser.c`.
fn binop(s: &str) -> Option<(u8, u8)> {
    Some(match s {
        "or" => (1, 1),
        "and" => (2, 2),
        "<" | ">" | "<=" | ">=" | "~=" | "==" => (3, 3),
        ".." => (5, 4),
        "+" | "-" => (6, 6),
        "*" | "/" | "%" => (7, 7),
        "^" => (10, 9),
        _ => return None,
    })
}
const UNARY: u8 = 8;

impl<'t> P<'t> {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].tok
    }
    fn span(&self) -> Span {
        self.toks[self.i.min(self.toks.len() - 1)].span
    }
    fn prev_end(&self) -> usize {
        self.i.checked_sub(1).map_or(0, |j| self.toks[j].span.end)
    }
    fn is(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }
    fn eat(&mut self, s: &str) -> bool {
        self.is(s).then(|| self.i += 1).is_some()
    }
    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        let near = match self.peek() {
            Tok::Eof => "<eof>".to_owned(),
            Tok::Name(n) => n.clone(),
            Tok::Sym(s) => (*s).to_owned(),
            Tok::Str(_) => "string".into(),
            Tok::Number(n) => n.to_string(),
            Tok::Capture(c) => format!("${c}"),
        };
        Err(ParseError {
            msg: format!("{} near '{near}'", msg.into()),
            span: self.span(),
        })
    }
    fn expect(&mut self, s: &str) -> R<()> {
        if self.eat(s) {
            Ok(())
        } else {
            self.err(format!("'{s}' expected"))
        }
    }
    fn name(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Name(n) => {
                self.i += 1;
                Ok(n)
            }
            _ => self.err("<name> expected"),
        }
    }

    fn block_end(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
            || ["end", "else", "elseif", "until"]
                .iter()
                .any(|s| self.is(s))
    }

    fn block(&mut self) -> R<Block> {
        let mut out = Vec::new();
        while !self.block_end() {
            if self.is("return") {
                let start = self.span().start;
                self.i += 1;
                let vals = match self.block_end() || self.is(";") {
                    true => Vec::new(),
                    false => self.exprlist()?,
                };
                self.eat(";");
                out.push(Stat::Return(
                    vals,
                    Span {
                        start,
                        end: self.prev_end(),
                    },
                ));
                break;
            }
            if let Some(s) = self.statement()? {
                out.push(s);
            }
        }
        Ok(out)
    }

    fn statement(&mut self) -> R<Option<Stat>> {
        let s = match self.peek().clone() {
            Tok::Sym(";") => {
                self.i += 1;
                return Ok(None);
            }
            Tok::Sym("if") => {
                self.i += 1;
                let mut arms = vec![(self.expr()?, {
                    self.expect("then")?;
                    self.block()?
                })];
                while self.eat("elseif") {
                    let c = self.expr()?;
                    self.expect("then")?;
                    arms.push((c, self.block()?));
                }
                let els = if self.eat("else") {
                    Some(self.block()?)
                } else {
                    None
                };
                self.expect("end")?;
                Stat::If(arms, els)
            }
            Tok::Sym("while") => {
                self.i += 1;
                let c = self.expr()?;
                self.expect("do")?;
                let b = self.block()?;
                self.expect("end")?;
                Stat::While(c, b)
            }
            Tok::Sym("do") => {
                self.i += 1;
                let b = self.block()?;
                self.expect("end")?;
                Stat::Do(b)
            }
            Tok::Sym("for") => {
                self.i += 1;
                let first = self.name()?;
                match self.eat("=") {
                    true => {
                        let a = self.expr()?;
                        self.expect(",")?;
                        let b = self.expr()?;
                        let step = if self.eat(",") {
                            Some(self.expr()?)
                        } else {
                            None
                        };
                        self.expect("do")?;
                        let body = self.block()?;
                        self.expect("end")?;
                        Stat::NumFor(first, a, b, step, body)
                    }
                    false => {
                        let mut names = vec![first];
                        while self.eat(",") {
                            names.push(self.name()?);
                        }
                        self.expect("in")?;
                        let exprs = self.exprlist()?;
                        self.expect("do")?;
                        let body = self.block()?;
                        self.expect("end")?;
                        Stat::GenFor(names, exprs, body)
                    }
                }
            }
            Tok::Sym("repeat") => {
                self.i += 1;
                let b = self.block()?;
                self.expect("until")?;
                Stat::Repeat(b, self.expr()?)
            }
            Tok::Sym("function") => {
                self.i += 1;
                let start = self.span();
                let mut target = Expr::Name(self.name()?, start);
                while self.eat(".") {
                    let f = self.name()?;
                    target = Expr::Field(
                        Box::new(target),
                        f,
                        Span {
                            start: start.start,
                            end: self.prev_end(),
                        },
                    );
                }
                let method = if self.eat(":") {
                    Some(self.name()?)
                } else {
                    None
                };
                let mut body = self.funcbody()?;
                if let Some(m) = method {
                    body.params.insert(0, "self".into());
                    target = Expr::Field(
                        Box::new(target),
                        m,
                        Span {
                            start: start.start,
                            end: self.prev_end(),
                        },
                    );
                }
                Stat::Function(target, Box::new(body))
            }
            Tok::Sym("local") => {
                self.i += 1;
                match self.eat("function") {
                    true => {
                        let n = self.name()?;
                        Stat::LocalFunction(n, Box::new(self.funcbody()?))
                    }
                    false => {
                        let mut names = vec![self.name()?];
                        while self.eat(",") {
                            names.push(self.name()?);
                        }
                        let vals = if self.eat("=") {
                            self.exprlist()?
                        } else {
                            Vec::new()
                        };
                        Stat::Local(names, vals)
                    }
                }
            }
            Tok::Sym("break") => {
                self.i += 1;
                Stat::Break
            }
            Tok::Sym("continue") => {
                self.i += 1;
                Stat::Continue
            }
            Tok::Sym("goto") => {
                self.i += 1;
                Stat::Goto(self.name()?)
            }
            Tok::Sym("::") => {
                self.i += 1;
                let n = self.name()?;
                self.expect("::")?;
                Stat::Label(n)
            }
            _ => {
                let e = self.suffixed()?;
                match self.is("=") || self.is(",") {
                    true => {
                        let mut targets = vec![e];
                        while self.eat(",") {
                            targets.push(self.suffixed()?);
                        }
                        self.expect("=")?;
                        if targets.iter().any(|t| {
                            !matches!(t, Expr::Name(..) | Expr::Index(..) | Expr::Field(..))
                        }) {
                            return self.err("syntax error (cannot assign)");
                        }
                        Stat::Assign(targets, self.exprlist()?)
                    }
                    false => match e {
                        Expr::Call(..) | Expr::Method(..) => Stat::Expr(e),
                        _ => return self.err("syntax error (statement expected)"),
                    },
                }
            }
        };
        Ok(Some(s))
    }

    fn funcbody(&mut self) -> R<FuncBody> {
        self.expect("(")?;
        let mut params = Vec::new();
        let mut vararg = false;
        if !self.is(")") {
            loop {
                if self.eat("...") {
                    vararg = true;
                    break;
                }
                params.push(self.name()?);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        let body = self.block()?;
        self.expect("end")?;
        Ok(FuncBody {
            params,
            vararg,
            body,
        })
    }

    fn exprlist(&mut self) -> R<Vec<Expr>> {
        let mut v = vec![self.expr()?];
        while self.eat(",") {
            v.push(self.expr()?);
        }
        Ok(v)
    }

    fn expr(&mut self) -> R<Expr> {
        self.subexpr(0)
    }

    fn subexpr(&mut self, limit: u8) -> R<Expr> {
        let mut left = match self.peek() {
            Tok::Sym(op @ ("not" | "-" | "#")) => {
                let op = *op;
                self.i += 1;
                Expr::Un(op, Box::new(self.subexpr(UNARY)?))
            }
            _ => self.simple()?,
        };
        while let Tok::Sym(op) = self.peek().clone() {
            match binop(op) {
                Some((l, r)) if l > limit => {
                    self.i += 1;
                    left = Expr::Bin(Box::new(left), op, Box::new(self.subexpr(r)?));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn simple(&mut self) -> R<Expr> {
        let e = match self.peek().clone() {
            Tok::Number(n) => Expr::Number(n),
            Tok::Str(s) => Expr::Str(s),
            Tok::Sym("nil") => Expr::Nil,
            Tok::Sym("true") => Expr::True,
            Tok::Sym("false") => Expr::False,
            Tok::Sym("...") => Expr::Vararg,
            Tok::Sym("{") => return self.table(),
            Tok::Sym("function") => {
                self.i += 1;
                return Ok(Expr::Function(Box::new(self.funcbody()?)));
            }
            _ => return self.suffixed(),
        };
        self.i += 1;
        Ok(e)
    }

    fn primary(&mut self) -> R<Expr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Name(n) => {
                self.i += 1;
                Ok(Expr::Name(n, sp))
            }
            Tok::Capture(c) => {
                self.i += 1;
                Ok(Expr::Capture(c, sp))
            }
            Tok::Sym("(") => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(Expr::Paren(Box::new(e)))
            }
            _ => self.err("unexpected symbol"),
        }
    }

    fn args(&mut self) -> R<Vec<Expr>> {
        match self.peek().clone() {
            Tok::Str(s) => {
                self.i += 1;
                Ok(vec![Expr::Str(s)])
            }
            Tok::Sym("{") => Ok(vec![self.table()?]),
            Tok::Sym("(") => {
                self.i += 1;
                let a = if self.is(")") {
                    Vec::new()
                } else {
                    self.exprlist()?
                };
                self.expect(")")?;
                Ok(a)
            }
            _ => self.err("function arguments expected"),
        }
    }

    fn suffixed(&mut self) -> R<Expr> {
        let start = self.span().start;
        let mut e = self.primary()?;
        loop {
            let sp = |p: &Self| Span {
                start,
                end: p.prev_end(),
            };
            e = match self.peek().clone() {
                Tok::Sym(".") => {
                    self.i += 1;
                    let f = self.name()?;
                    Expr::Field(Box::new(e), f, sp(self))
                }
                Tok::Sym("[") => {
                    self.i += 1;
                    let k = self.expr()?;
                    self.expect("]")?;
                    Expr::Index(Box::new(e), Box::new(k), sp(self))
                }
                Tok::Sym(":") => {
                    self.i += 1;
                    let m = self.name()?;
                    let a = self.args()?;
                    Expr::Method(Box::new(e), m, a, sp(self))
                }
                Tok::Sym("(" | "{") | Tok::Str(_) => {
                    let a = self.args()?;
                    Expr::Call(Box::new(e), a, sp(self))
                }
                _ => return Ok(e),
            };
        }
    }

    fn table(&mut self) -> R<Expr> {
        self.expect("{")?;
        let mut items = Vec::new();
        while !self.is("}") {
            let item = match (
                self.peek().clone(),
                self.toks.get(self.i + 1).map(|t| &t.tok),
            ) {
                (Tok::Sym("["), _) => {
                    self.i += 1;
                    let k = self.expr()?;
                    self.expect("]")?;
                    self.expect("=")?;
                    (Some(k), self.expr()?)
                }
                (Tok::Name(n), Some(Tok::Sym("="))) => {
                    self.i += 2;
                    (Some(Expr::Str(n)), self.expr()?)
                }
                _ => (None, self.expr()?),
            };
            items.push(item);
            if !(self.eat(",") || self.eat(";")) {
                break;
            }
        }
        self.expect("}")?;
        Ok(Expr::Table(items))
    }
}

/// Parses a chunk.
pub fn parse(toks: &[Token]) -> Result<Block, ParseError> {
    let mut p = P { toks, i: 0 };
    let b = p.block()?;
    match p.peek() {
        Tok::Eof => Ok(b),
        _ => p.err("'<eof>' expected"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn ok(s: &str) -> Block {
        parse(&lex(s).unwrap()).unwrap_or_else(|e| panic!("{s}: {e:?}"))
    }

    #[test]
    fn parses_glua() {
        ok(
            "local n = 0 for _, e in ipairs(ents.FindByClass('prop')) do if e:GetOwner() == $ply then n = n + 1 end end return math.min(n, $limit)",
        );
        ok("for i = 1, 10 do if i != 2 && !false || x then continue end end");
        ok("local t = { 1, a = 2, ['b'] = 3; f = function(...) return ... end }");
        ok("function a.b.c:d(x) self.x = x end local function f() end goto l ::l::");
        ok("repeat local x = 1 until x > 0 print'hi' f{1} a.b[c](1)(2)");
        ok("return");
    }

    #[test]
    fn precedence() {
        let b = ok("return 1 + 2 * 3 .. 'x' .. 'y'");
        match &b[0] {
            Stat::Return(v, _) => assert!(
                matches!(&v[0], Expr::Bin(_, "..", r) if matches!(**r, Expr::Bin(_, "..", _)))
            ),
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn errors() {
        for bad in ["if x then", "x = ", "local = 1", "f(", "1 + 1", "a.b"] {
            assert!(parse(&lex(bad).unwrap()).is_err(), "{bad}");
        }
        let e = parse(&lex("local x = (1").unwrap()).unwrap_err();
        assert!(e.msg.contains("')' expected"), "{}", e.msg);
    }
}
