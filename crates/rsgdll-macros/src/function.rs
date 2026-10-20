//! `#[rsgdll::function]` (spec §8.2) and lifecycle attributes (spec §8.1).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{FnArg, ItemFn, LitInt, LitStr, ReturnType, Type, spanned::Spanned};

use crate::util::{err, is_lua_ref, last_ident, pascal, realm_guard, realm_tokens, rt, shadows_global, slice_attrs};

#[derive(Default)]
pub struct FnAttr {
    pub name: Option<LitStr>,
    pub realm: Option<LitStr>,
    pub overwrite: bool,
}

pub fn parse_fn_attr(attr: TokenStream) -> syn::Result<FnAttr> {
    let mut a = FnAttr::default();
    let p = syn::meta::parser(|m| {
        match m.path.get_ident().map(ToString::to_string).as_deref() {
            Some("name") => a.name = Some(m.value()?.parse()?),
            Some("realm") => a.realm = Some(m.value()?.parse()?),
            Some("overwrite") => a.overwrite = true,
            _ => return Err(m.error("expected `name`, `realm` or `overwrite`")),
        }
        Ok(())
    });
    syn::parse::Parser::parse2(p, attr)?;
    Ok(a)
}

/// How a user fn's return value becomes an `Outcome` (decided syntactically).
pub enum RetKind {
    /// `T`, `()`.
    Value,
    /// `Result<T>` / `Result<T, rsgdll::Error>`: keeps the backtrace.
    RsgdllResult,
    /// `Result<T, E: Display>`.
    DisplayResult,
}

pub fn ret_kind(ret: &ReturnType) -> RetKind {
    let ReturnType::Type(_, ty) = ret else { return RetKind::Value };
    let Type::Path(p) = &**ty else { return RetKind::Value };
    let Some(seg) = p.path.segments.last().filter(|s| s.ident == "Result") else { return RetKind::Value };
    match &seg.arguments {
        syn::PathArguments::AngleBracketed(a) if a.args.len() >= 2 => match a.args.iter().nth(1) {
            Some(syn::GenericArgument::Type(Type::Path(e))) if e.path.segments.last().is_some_and(|s| s.ident == "Error") => RetKind::RsgdllResult,
            _ => RetKind::DisplayResult,
        },
        _ => RetKind::RsgdllResult,
    }
}

/// Expression turning `__r` into an `Outcome`.
pub fn outcome(kind: &RetKind, name: &TokenStream, in_module: bool) -> TokenStream {
    let rt = rt();
    let ok = quote!(#rt::export::value(__lua, __count, #name, #in_module, __v));
    match kind {
        RetKind::Value => quote! { { let __v = __r; #ok } },
        RetKind::RsgdllResult => quote! {
            match __r {
                ::core::result::Result::Ok(__v) => #ok,
                ::core::result::Result::Err(__e) => #rt::export::error(__lua, #name, #in_module, #rt::Error::from(__e).lua_message()),
            }
        },
        RetKind::DisplayResult => quote! {
            match __r {
                ::core::result::Result::Ok(__v) => #ok,
                ::core::result::Result::Err(__e) => #rt::export::error(__lua, #name, #in_module, ::std::string::ToString::to_string(&__e)),
            }
        },
    }
}

/// Argument conversion statements + call argument list for a user fn.
/// `first_lua_arg` = Lua stack position of the first converted param (1 for exports).
pub fn convert_args<'a>(inputs: impl Iterator<Item = &'a FnArg>, first_lua_arg: i32) -> syn::Result<(Vec<TokenStream>, Vec<TokenStream>)> {
    let rt = rt();
    let mut stmts = Vec::new();
    let mut call = Vec::new();
    let params: Vec<&syn::PatType> = inputs
        .map(|a| match a {
            FnArg::Typed(t) => Ok(t),
            FnArg::Receiver(r) => Err(err(r.span(), "`self` is not allowed here")),
        })
        .collect::<syn::Result<_>>()?;
    let mut pos = 0i32;
    params.iter().enumerate().try_for_each(|(i, p)| -> syn::Result<()> {
        let v = format_ident!("__a{}", i);
        let ty = &p.ty;
        match (is_lua_ref(ty), last_ident(ty).as_deref()) {
            (true, _) if i == 0 => call.push(quote!(__lua)),
            (true, _) => return Err(err(ty.span(), "`&Lua` must be the first parameter")),
            (false, Some("Variadic")) => {
                if i + 1 != params.len() {
                    return Err(err(ty.span(), "`Variadic<T>` must be the last parameter"));
                }
                stmts.push(quote_spanned! {ty.span()=> let #v: #ty = #rt::conv::multi::variadic_from(__lua, #first_lua_arg, __count - #first_lua_arg + 1, #pos)?; });
                call.push(quote!(#v));
            }
            (false, _) => {
                stmts.push(quote_spanned! {ty.span()=> let #v: #ty = #rt::conv::multi::arg(__lua, #first_lua_arg, __count - #first_lua_arg + 1, #pos)?; });
                call.push(quote!(#v));
                pos += 1;
            }
        }
        Ok(())
    })?;
    Ok((stmts, call))
}

pub fn function(attr: TokenStream, item: ItemFn) -> syn::Result<TokenStream> {
    let a = parse_fn_attr(attr)?;
    if item.sig.asyncness.is_some() {
        return crate::asyncfn::async_function(a, item);
    }
    let rt = rt();
    let ident = &item.sig.ident;
    let (name, in_module) = match &a.name {
        Some(n) => (n.value(), false),
        None => (pascal(&ident.to_string()), true),
    };
    if !a.overwrite && !in_module && shadows_global(&name) {
        return Err(err(
            a.name.as_ref().map_or(ident.span(), |n| n.span()),
            format!("export name `{name}` shadows a GMod global; add `overwrite` to replace it"),
        ));
    }
    let (stmts, call) = convert_args(item.sig.inputs.iter(), 1)?;
    let name_lit = LitStr::new(&name, Span::call_site());
    let name_ts = quote!(#name_lit);
    let out = outcome(&ret_kind(&item.sig.output), &name_ts, in_module);
    let realm = match &a.realm {
        Some(r) => {
            let t = realm_tokens(r.span(), &r.value())?;
            quote!(::core::option::Option::Some(#t))
        }
        None => quote!(::core::option::Option::None),
    };
    let guard = a.realm.as_ref().map(|r| realm_guard(r.span(), &r.value(), &format!("export `{name}`")));
    let slice = slice_attrs("EXPORTS");
    let tramp = format_ident!("__rsgdll_tramp_{}", ident);
    Ok(quote! {
        #item
        #guard
        #[doc(hidden)]
        #[allow(non_snake_case)]
        extern "C-unwind" fn #tramp(__l: *mut #rt::sys::lua_State) -> ::core::ffi::c_int {
            #rt::export::entry(__l, #name_lit, #in_module, &|__lua, __count| {
                #(#stmts)*
                let __r = #ident(#(#call),*);
                ::core::result::Result::Ok(#out)
            })
        }
        const _: () = {
            #slice
            static __E: #rt::module::Export = #rt::module::Export {
                name: #name_lit,
                in_module: #in_module,
                realm: #realm,
                func: #tramp,
            };
        };
    })
}

/// `#[preopen]`, `#[postopen]`, `#[preclose]`, `#[postclose]` with optional `order = N`.
pub fn lifecycle(slice: &str, attr: TokenStream, item: ItemFn) -> syn::Result<TokenStream> {
    let mut order = 0i32;
    let p = syn::meta::parser(|m| {
        if m.path.is_ident("order") {
            order = m.value()?.parse::<LitInt>()?.base10_parse()?;
            Ok(())
        } else {
            Err(m.error("expected `order = N`"))
        }
    });
    syn::parse::Parser::parse2(p, attr)?;
    let rt = rt();
    let ident = &item.sig.ident;
    let name = ident.to_string();
    let sl = slice_attrs(slice);
    let ty = match slice {
        "PREOPEN" | "POSTOPEN" => quote!(fn(&#rt::Lua) -> #rt::Result<()>),
        "PRECLOSE" => quote!(fn(&#rt::Lua)),
        _ => quote!(fn()),
    };
    // Adapt `fn(&Lua) -> Result<()>` / `fn() -> ...` shapes by wrapping.
    let n = item.sig.inputs.len();
    let wrapper = match (slice, n) {
        ("PREOPEN" | "POSTOPEN", 1) => quote!(#ident),
        ("PREOPEN" | "POSTOPEN", 0) => quote!({ fn __w(_: &#rt::Lua) -> #rt::Result<()> { #ident() } __w }),
        ("PRECLOSE", 1) => quote!(#ident),
        ("PRECLOSE", 0) => quote!({ fn __w(_: &#rt::Lua) { #ident() } __w }),
        ("POSTCLOSE", 0) => quote!(#ident),
        _ => return Err(err(item.sig.span(), "unexpected signature for this lifecycle hook")),
    };
    Ok(quote! {
        #item
        const _: () = {
            #sl
            static __H: #rt::module::Ordered<#ty> = #rt::module::Ordered { order: #order, name: #name, f: #wrapper };
        };
    })
}
