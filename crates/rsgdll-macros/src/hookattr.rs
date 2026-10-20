//! `#[rsgdll::hook(Ident | "Name" [, unchecked])]` and `#[rsgdll::timer(...)]`
//! (spec §10.3/10.4). Registered at open (REGISTER slice), removed at close.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{FnArg, Ident, ItemFn, LitInt, LitStr, ReturnType, Type, spanned::Spanned};

use crate::function::{RetKind, convert_args, ret_kind};
use crate::util::{err, is_lua_ref, realm_guard, rt, slice_attrs};

enum Target {
    Builtin(Ident),
    Custom(LitStr),
}

/// `()` / absent / `Result<()>` returns mean "no override".
fn returns_unit(ret: &ReturnType) -> bool {
    let unit = |t: &Type| matches!(t, Type::Tuple(t) if t.elems.is_empty());
    match ret {
        ReturnType::Default => true,
        ReturnType::Type(_, t) if unit(t) => true,
        ReturnType::Type(_, t) => match &**t {
            Type::Path(p) => p.path.segments.last().is_some_and(|s| {
                s.ident == "Result"
                    && matches!(&s.arguments, syn::PathArguments::AngleBracketed(a)
                        if matches!(a.args.first(), Some(syn::GenericArgument::Type(t)) if unit(t)))
            }),
            _ => false,
        },
    }
}

fn register(name: &str, body: TokenStream) -> TokenStream {
    let rt = rt();
    let slice = slice_attrs("REGISTER");
    quote! {
        const _: () = {
            fn __reg(__lua: &#rt::Lua) -> #rt::Result<()> { #body }
            #slice
            static __R: #rt::module::Ordered<fn(&#rt::Lua) -> #rt::Result<()>> =
                #rt::module::Ordered { order: 0, name: #name, f: __reg };
        };
    }
}

pub fn hook(attr: TokenStream, item: ItemFn) -> syn::Result<TokenStream> {
    let rt = rt();
    // A string literal is not a meta path; try it first.
    let (target, unchecked) = match syn::parse2::<LitStr>(attr.clone()) {
        Ok(l) => (Target::Custom(l), false),
        Err(_) => {
            let mut target = None;
            let mut unchecked = false;
            let p = syn::meta::parser(|m| {
                if m.path.is_ident("unchecked") {
                    unchecked = true;
                } else if let Some(id) = m.path.get_ident() {
                    target = Some(Target::Builtin(id.clone()));
                }
                Ok(())
            });
            syn::parse::Parser::parse2(p, attr)?;
            let t = target.ok_or_else(|| {
                err(
                    Span::call_site(),
                    "expected a hook name: `Think` or \"Custom.Event\"",
                )
            })?;
            (t, unchecked)
        }
    };
    let ident = &item.sig.ident;
    let fname = ident.to_string();
    let takes_lua = matches!(item.sig.inputs.first(), Some(FnArg::Typed(t)) if is_lua_ref(&t.ty));
    let params: Vec<&FnArg> = item
        .sig
        .inputs
        .iter()
        .skip(usize::from(takes_lua))
        .collect();
    let unit = returns_unit(&item.sig.output);
    let kind = ret_kind(&item.sig.output);
    let (event, body, guard) = match (&target, unchecked) {
        (Target::Builtin(h), false) => {
            let name = h.to_string();
            let db = rsgdll_apidb::db();
            let found = db.hook("GM", &name).or_else(|| db.hook("SANDBOX", &name));
            let Some(found) = found else {
                let all = db.hooks.values().map(|h| h.name.as_str());
                let hint = rsgdll_luacheck::suggest(&name, all)
                    .map_or(String::new(), |s| format!(" (did you mean `{s}`?)"));
                return Err(err(
                    h.span(),
                    format!("unknown GMod hook `{name}`{hint}; use a string for custom hooks"),
                ));
            };
            let path = match found.owner.as_str() {
                "SANDBOX" => quote!(#rt::gmod::hooks::sandbox::#h),
                _ => quote!(#rt::gmod::hooks::#h),
            };
            let tys: Vec<&Type> = params
                .iter()
                .filter_map(|a| match a {
                    FnArg::Typed(t) => Some(&*t.ty),
                    _ => None,
                })
                .collect();
            let idx: Vec<syn::Index> = (0..tys.len()).map(syn::Index::from).collect();
            let guard = realm_guard(h.span(), found.realm.as_str(), &format!("hook `{name}`"));
            let (stmts, call) = convert_args(item.sig.inputs.iter(), 1)?;
            let name_lit = LitStr::new(&format!("hook {name}"), Span::call_site());
            let out = if unit {
                let ok = quote!(#rt::export::value(__lua, __count, #name_lit, false, ()));
                match kind {
                    RetKind::Value => quote! { { let _ = __r; #ok } },
                    _ => quote! { match __r {
                        ::core::result::Result::Ok(_) => #ok,
                        ::core::result::Result::Err(__e) => #rt::export::error(__lua, #name_lit, false, #rt::Error::from(__e).lua_message()),
                    } },
                }
            } else {
                crate::function::outcome(&kind, &quote!(#name_lit), false)
            };
            let body = quote! {
                // Type check: each param must be `HookArgCompat` with the hook's arg at that position (rustc errors
                // on mismatch; an extra param indexes past the tuple and fails to compile).
                #[allow(dead_code, unused_variables)]
                fn __check<'a>(__a: <#path as #rt::gmod::hook::Hook>::Args<'a>) {
                    #( #rt::gmod::hook::compat::<#tys, _>(&__a.#idx); )*
                }
                let __id = ::std::format!("rsgdll.{}.{}", __lua.module_name(), #fname);
                #rt::gmod::hook::add_raw(__lua, <#path as #rt::gmod::hook::Hook>::NAME, &__id, |__lua, __count| {
                    #(#stmts)*
                    let __r = #ident(#(#call),*);
                    ::core::result::Result::Ok(#out)
                })
            };
            (
                name,
                body,
                (found.realm != rsgdll_apidb::Realm::Shared).then_some(guard),
            )
        }
        (t, _) => {
            let name = match t {
                Target::Builtin(i) => i.to_string(),
                Target::Custom(l) => l.value(),
            };
            let (stmts, call) = convert_args(item.sig.inputs.iter(), 1)?;
            let name_lit = LitStr::new(&format!("hook {name}"), Span::call_site());
            let out = crate::function::outcome(&kind, &quote!(#name_lit), false);
            let body = quote! {
                let __id = ::std::format!("rsgdll.{}.{}", __lua.module_name(), #fname);
                #rt::gmod::hook::add_raw(__lua, #name, &__id, |__lua, __count| {
                    #(#stmts)*
                    let __r = #ident(#(#call),*);
                    ::core::result::Result::Ok(#out)
                })
            };
            (name, body, None)
        }
    };
    let reg = register(&format!("hook {event}"), body);
    Ok(quote! { #item #guard #reg })
}

/// Compile-time duration parse (`"1s"`, `"500ms"`, `"2m"`).
fn seconds(l: &LitStr) -> syn::Result<f64> {
    let s = l.value();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (n, unit) = s.split_at(split);
    let n: f64 = n
        .parse()
        .map_err(|_| err(l.span(), "expected a duration like \"1s\" or \"500ms\""))?;
    Ok(n * match unit {
        "ms" => 0.001,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        _ => return Err(err(l.span(), "duration unit must be ms, s, m or h")),
    })
}

pub fn timer(attr: TokenStream, item: ItemFn) -> syn::Result<TokenStream> {
    let rt = rt();
    let (mut every, mut delay, mut repeat) = (None, None, None::<u32>);
    let p = syn::meta::parser(|m| {
        match m.path.get_ident().map(ToString::to_string).as_deref() {
            Some("every") => every = Some(seconds(&m.value()?.parse()?)?),
            Some("delay") => delay = Some(seconds(&m.value()?.parse()?)?),
            Some("repeat") => repeat = Some(m.value()?.parse::<LitInt>()?.base10_parse()?),
            _ => return Err(m.error("expected `every = \"1s\"` or `delay = \"..\", repeat = N`")),
        }
        Ok(())
    });
    syn::parse::Parser::parse2(p, attr)?;
    let (secs, rep) = match (every, delay, repeat) {
        (Some(s), None, None) => (s, quote!(#rt::gmod::timer::Repeat::Forever)),
        (None, Some(s), r) => {
            let n = r.unwrap_or(1);
            (s, quote!(#rt::gmod::timer::Repeat::Times(#n)))
        }
        _ => {
            return Err(err(
                item.sig.span(),
                "use `every = \"..\"` or `delay = \"..\"[, repeat = N]`",
            ));
        }
    };
    let ident = &item.sig.ident;
    let fname = ident.to_string();
    let call = match item.sig.inputs.len() {
        0 => quote!(#ident()),
        _ => quote!(#ident(__lua)),
    };
    let call = match (returns_unit(&item.sig.output), ret_kind(&item.sig.output)) {
        (_, RetKind::Value) => quote!({ #call; ::core::result::Result::Ok(()) }),
        _ => quote!(#call.map_err(#rt::Error::from)),
    };
    let body = quote! {
        let __name = ::std::format!("rsgdll.{}.{}", __lua.module_name(), #fname);
        #rt::gmod::timer::create(__lua, &__name, ::core::time::Duration::from_secs_f64(#secs), #rep, |__lua| #call)
    };
    let reg = register(&format!("timer {fname}"), body);
    Ok(quote! { #item #reg })
}
