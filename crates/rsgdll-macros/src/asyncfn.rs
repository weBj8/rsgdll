//! Async `#[rsgdll::function]` (spec §10.2): args converted synchronously, the callback
//! `function(err, ...)` is the last Lua argument (or a `#[lua(callback)]` param), and the
//! future runs on the per-state executor.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{FnArg, ItemFn, LitStr, ReturnType, spanned::Spanned};

use crate::function::{FnAttr, RetKind, ret_kind};
use crate::util::{err, is_lua_ref, last_ident, pascal, rt, slice_attrs};

pub fn async_function(a: FnAttr, mut item: ItemFn) -> syn::Result<TokenStream> {
    let rt = rt();
    let ident = item.sig.ident.clone();
    let (name, in_module) = match &a.name {
        Some(n) => (n.value(), false),
        None => (pascal(&ident.to_string()), true),
    };
    let mut explicit_cb: Option<usize> = None;
    item.sig.inputs.iter_mut().enumerate().for_each(|(i, arg)| {
        if let FnArg::Typed(t) = arg {
            let before = t.attrs.len();
            t.attrs.retain(|at| {
                !(at.path().is_ident("lua")
                    && at
                        .parse_args::<syn::Ident>()
                        .is_ok_and(|id| id == "callback"))
            });
            if t.attrs.len() != before {
                explicit_cb = Some(i);
            }
        }
    });
    let params: Vec<&syn::PatType> = item
        .sig
        .inputs
        .iter()
        .map(|a| match a {
            FnArg::Typed(t) => Ok(t),
            FnArg::Receiver(r) => Err(err(r.span(), "`self` is not allowed here")),
        })
        .collect::<syn::Result<_>>()?;
    let kinds: Vec<Option<String>> = params.iter().map(|p| last_ident(&p.ty)).collect();
    if kinds.iter().any(|k| k.as_deref() == Some("Variadic")) {
        return Err(err(
            item.sig.span(),
            "`Variadic<T>` is not allowed in async exports",
        ));
    }
    let ambiguous = kinds.iter().enumerate().any(|(i, k)| {
        Some(i) != explicit_cb && matches!(k.as_deref(), Some("Option" | "LuaFunction"))
    });
    if ambiguous && explicit_cb.is_none() {
        return Err(err(
            item.sig.span(),
            "async export with `Option`/`LuaFunction` params must mark the callback with `#[lua(callback)] cb: Callback`",
        ));
    }
    let mut stmts = Vec::new();
    let mut call = Vec::new();
    let mut pos = 0i32;
    params.iter().enumerate().try_for_each(|(i, p)| -> syn::Result<()> {
        let v = format_ident!("__a{}", i);
        let ty = &p.ty;
        match (Some(i) == explicit_cb, is_lua_ref(ty), kinds[i].as_deref()) {
            (true, _, _) => {
                stmts.push(quote_spanned! {ty.span()=> let __cb_pos: i32 = #pos; });
                pos += 1;
            }
            (_, true, _) => return Err(err(ty.span(), "async exports take `lua: LocalLua`, not `&Lua`")),
            (_, _, Some("LocalLua")) => call.push(quote!(#rt::rt::LocalLua::new(__lua))),
            _ => {
                stmts.push(quote_spanned! {ty.span()=> let #v: #ty = #rt::conv::multi::arg(__lua, 1, __count, #pos)?; });
                call.push(quote!(#v));
                pos += 1;
            }
        }
        Ok(())
    })?;
    let cb_pos = match explicit_cb {
        Some(_) => quote!(__cb_pos),
        None => quote!(#pos),
    };
    let name_lit = LitStr::new(&name, Span::call_site());
    let ret = match (&item.sig.output, ret_kind(&item.sig.output)) {
        (ReturnType::Default, _) => quote!(::core::result::Result::<(), #rt::Error>::Ok(__f.await)),
        (_, RetKind::Value) => quote!(::core::result::Result::<_, #rt::Error>::Ok(__f.await)),
        (_, RetKind::RsgdllResult) => quote!(__f.await.map_err(#rt::Error::from)),
        (_, RetKind::DisplayResult) => quote!(__f.await.map_err(#rt::Error::msg)),
    };
    let slice = slice_attrs("EXPORTS");
    let tramp = format_ident!("__rsgdll_tramp_{}", ident);
    let realm = match &a.realm {
        Some(r) => {
            let t = crate::util::realm_tokens(r.span(), &r.value())?;
            quote!(::core::option::Option::Some(#t))
        }
        None => quote!(::core::option::Option::None),
    };
    Ok(quote! {
        #item
        #[doc(hidden)]
        #[allow(non_snake_case)]
        extern "C-unwind" fn #tramp(__l: *mut #rt::sys::lua_State) -> ::core::ffi::c_int {
            #rt::export::entry(__l, #name_lit, #in_module, &|__lua, __count| {
                #(#stmts)*
                let __cb: #rt::conv::LuaFunction = #rt::conv::multi::arg(__lua, 1, __count, #cb_pos)?;
                let __f = #ident(#(#call),*);
                #rt::export::spawn_async(__lua, #name_lit, #in_module, __cb, async move { #ret });
                ::core::result::Result::Ok(#rt::trampoline::Outcome::Ok(0))
            })
        }
        const _: () = {
            #slice
            static __E: #rt::module::Export = #rt::module::Export { name: #name_lit, in_module: #in_module, realm: #realm, func: #tramp };
        };
    })
}
