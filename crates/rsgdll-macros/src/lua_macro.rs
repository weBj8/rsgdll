//! `lua!(handle, [unchecked,] "chunk" | tokens, [name = expr, ...])` (spec §8.3, §9.2).

use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use rsgdll_luacheck::{Ctx, Kind, Severity};
use syn::{Expr, Ident, LitStr, Token, parse::Parse, parse::ParseStream};

use crate::util::{realm_guard, rt, warning};

enum Body {
    Str(LitStr),
    /// Rebuilt source + (byte offset, span) of each token.
    Tokens(String, Vec<(usize, Span)>),
}

pub struct LuaInput {
    handle: Expr,
    unchecked: bool,
    body: Body,
    named: Vec<(Ident, Expr)>,
}

/// Rebuilds Lua source from Rust tokens (`Spacing::Joint` rejoins `~=`, `..`, `::`).
fn rebuild(ts: TokenStream, src: &mut String, spans: &mut Vec<(usize, Span)>) -> syn::Result<()> {
    let mut it = ts.into_iter().peekable();
    while let Some(tt) = it.next() {
        match tt {
            TokenTree::Group(g) => {
                let (o, c) = match g.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                spans.push((src.len(), g.span_open()));
                src.push_str(o);
                rebuild(g.stream(), src, spans)?;
                spans.push((src.len(), g.span_close()));
                src.push_str(c);
                src.push(' ');
            }
            TokenTree::Punct(p) if p.as_char() == '$' => {
                let next = it.next();
                match next {
                    Some(TokenTree::Ident(id)) => {
                        spans.push((src.len(), p.span()));
                        src.push('$');
                        src.push_str(&id.to_string());
                        src.push(' ');
                    }
                    _ => return Err(syn::Error::new(p.span(), "expected identifier after `$`")),
                }
            }
            TokenTree::Punct(p) => {
                let next_dash = matches!(it.peek(), Some(TokenTree::Punct(n)) if n.as_char() == '-');
                if p.as_char() == '-' && p.spacing() == Spacing::Joint && next_dash {
                    return Err(syn::Error::new(p.span(), "`--` comments are not available in the token form; use the string form `lua!(lua, r#\"...\"#)`"));
                }
                spans.push((src.len(), p.span()));
                src.push(p.as_char());
                if p.spacing() == Spacing::Alone {
                    src.push(' ');
                }
            }
            TokenTree::Ident(id) => {
                spans.push((src.len(), id.span()));
                src.push_str(&id.to_string());
                src.push(' ');
            }
            TokenTree::Literal(l) => {
                let text = l.to_string();
                let ok = text.starts_with('"') || text.chars().next().is_some_and(|c| c.is_ascii_digit());
                if !ok {
                    return Err(syn::Error::new(l.span(), "only double-quoted strings and plain numbers work in the token form; use the string form `lua!(lua, r#\"...\"#)`"));
                }
                spans.push((src.len(), l.span()));
                src.push_str(&text);
                src.push(' ');
            }
        }
    }
    Ok(())
}

impl Parse for LuaInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let handle: Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        let unchecked = input.peek(Ident) && input.peek2(Token![,]) && input.fork().parse::<Ident>().is_ok_and(|i| i == "unchecked");
        if unchecked {
            input.parse::<Ident>()?;
            input.parse::<Token![,]>()?;
        }
        // String form: a single literal followed by end or `, name = expr`.
        let is_str = input.peek(LitStr) && (input.peek2(Token![,]) || {
            let f = input.fork();
            f.parse::<LitStr>().is_ok() && f.is_empty()
        });
        let (body, named) = match is_str {
            true => {
                let lit: LitStr = input.parse()?;
                let mut named = Vec::new();
                while input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
                    let n: Ident = input.parse()?;
                    input.parse::<Token![=]>()?;
                    named.push((n, input.parse()?));
                }
                (Body::Str(lit), named)
            }
            false => {
                let ts: TokenStream = input.parse()?;
                let (mut src, mut spans) = (String::new(), Vec::new());
                rebuild(ts, &mut src, &mut spans)?;
                (Body::Tokens(src, spans), Vec::new())
            }
        };
        Ok(LuaInput { handle, unchecked, body, named })
    }
}

impl LuaInput {
    fn source(&self) -> String {
        match &self.body {
            Body::Str(l) => l.value(),
            Body::Tokens(s, _) => s.clone(),
        }
    }

    /// Best span for a byte offset in the source.
    fn span_at(&self, offset: usize) -> Span {
        match &self.body {
            // Sub-spans of literals need nightly; stable points at the whole literal.
            Body::Str(l) => l.span(),
            Body::Tokens(_, spans) => spans.iter().rev().find(|(o, _)| *o <= offset).map_or_else(Span::call_site, |(_, s)| *s),
        }
    }
}

pub fn lua(input: LuaInput) -> syn::Result<TokenStream> {
    let rt = rt();
    let src = input.source();
    // An expression chunk returns its value (`let n: i32 = lua!(lua, #player.GetAll())?`).
    let runnable = match rsgdll_luacheck::is_expression(&src) {
        true => format!("return {src}"),
        false => src.clone(),
    };
    let (rewritten, captures) = rsgdll_luacheck::rewrite(&runnable).map_err(|d| syn::Error::new(input.span_at(d.span.start), d.msg))?;
    // shortcut: capture Rust types are invisible to a proc macro, so method checks only
    // apply to known-return calls; upgrade by accepting `name: Type = expr` hints.
    let ctx = Ctx { captures: captures.iter().map(|c| (c.clone(), None)).collect(), ..Ctx::default() };
    let diags = rsgdll_luacheck::check(&src, rsgdll_apidb::db(), &ctx);
    let silenced = |k: &Kind| input.unchecked && matches!(k, Kind::UnknownMember | Kind::Arity | Kind::Realm(_) | Kind::Deprecated);
    let mut errors = Vec::new();
    let mut extra = Vec::new();
    diags.iter().filter(|d| !silenced(&d.kind)).for_each(|d| {
        let span = input.span_at(d.span.start);
        let (line, col) = rsgdll_luacheck::line_col(&src, d.span.start);
        let msg = match &input.body {
            Body::Str(_) => format!("lua!:{}:{}: {}", line + 1, col + 1, d.msg),
            Body::Tokens(..) => format!("lua!: {}", d.msg),
        };
        match (&d.kind, d.severity) {
            (Kind::Realm(r), _) => extra.push(realm_guard(span, r.as_str(), d.msg.split(" is ").next().unwrap_or(&d.msg))),
            (_, Severity::Error) => errors.push(syn::Error::new(span, msg)),
            (_, Severity::Warning) => extra.push(warning(span, &msg)),
        }
    });
    if let Some(e) = errors.into_iter().reduce(|mut a, b| {
        a.combine(b);
        a
    }) {
        return Err(e);
    }
    let lua51 = rsgdll_luacheck::desugar::desugar(&rewritten).map_err(|d| syn::Error::new(input.span_at(0), d.msg))?;
    let exprs: Vec<TokenStream> = captures
        .iter()
        .map(|c| match input.named.iter().find(|(n, _)| n == c) {
            Some((_, e)) => quote!(&(#e)),
            None => {
                let id = format_ident!("{}", c, span = input.span_at(0));
                quote!(&#id)
            }
        })
        .collect();
    let handle = &input.handle;
    let hspan = input.span_at(0);
    Ok(quote_spanned! {hspan=>
        {
            #(#extra)*
            static __RSGDLL_SITE: u8 = 0;
            const __RSGDLL_NAME: &::core::ffi::CStr = match ::core::ffi::CStr::from_bytes_with_nul(
                ::core::concat!("=", ::core::file!(), ":", ::core::line!(), ":", ::core::column!(), "\0").as_bytes()
            ) {
                ::core::result::Result::Ok(c) => c,
                ::core::result::Result::Err(_) => c"=lua!",
            };
            match #rt::chunk::AsLua::as_lua(&#handle) {
                ::core::result::Result::Ok(__l) => #rt::chunk::run(__l, &__RSGDLL_SITE, #lua51, __RSGDLL_NAME, #rt::chunk::Captures((#(#exprs,)*))),
                ::core::result::Result::Err(__e) => ::core::result::Result::Err(__e),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_form_rebuilds_operators() {
        let ts: TokenStream = "print(\"a\" .. $name, x ~= y, #t, a::b)".parse().unwrap();
        let (mut src, mut spans) = (String::new(), Vec::new());
        rebuild(ts, &mut src, &mut spans).unwrap();
        assert!(src.contains("..") && src.contains("~=") && src.contains("$name") && src.contains("::"), "{src}");
        let bad: TokenStream = "x -- y".parse().unwrap();
        assert!(rebuild(bad, &mut String::new(), &mut Vec::new()).is_err());
    }
}
