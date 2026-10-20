//! `#[rsgdll::methods] impl T { ... }` (spec §8.2): registers `&self` / `&mut self`
//! methods and `__*` metamethods for a `#[derive(LuaUserData)]` type.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, spanned::Spanned};

use crate::function::{RetKind, ret_kind};
use crate::util::{err, is_lua_ref, pascal, rt};

pub fn methods(item: ItemImpl) -> syn::Result<TokenStream> {
    let rt = rt();
    let ty = &item.self_ty;
    let regs = item
        .items
        .iter()
        .filter_map(|i| match i {
            ImplItem::Fn(f) => Some(f),
            _ => None,
        })
        .filter(|f| f.attrs.iter().all(|a| !a.path().is_ident("lua_skip")))
        .map(|f| -> syn::Result<Option<TokenStream>> {
            let recv = match f.sig.inputs.first() {
                Some(FnArg::Receiver(r)) => r,
                _ => return Ok(None), // associated fns are not methods
            };
            let ident = &f.sig.ident;
            let s = ident.to_string();
            let lua_name = if s.starts_with("__") { s.clone() } else { pascal(&s) };
            let rest: Vec<&FnArg> = f.sig.inputs.iter().skip(1).collect();
            let takes_lua = rest.first().is_some_and(|a| matches!(a, FnArg::Typed(t) if is_lua_ref(&t.ty)));
            let params: Vec<&syn::Type> = rest
                .iter()
                .skip(usize::from(takes_lua))
                .map(|a| match a {
                    FnArg::Typed(t) => Ok(&*t.ty),
                    FnArg::Receiver(r) => Err(err(r.span(), "unexpected receiver")),
                })
                .collect::<syn::Result<_>>()?;
            let names: Vec<_> = (0..params.len()).map(|i| quote::format_ident!("__p{}", i)).collect();
            let lua_arg = takes_lua.then(|| quote!(__lua,));
            let call = quote!(__this.#ident(#lua_arg #(#names),*));
            let body = match ret_kind(&f.sig.output) {
                RetKind::Value => quote!(::core::result::Result::Ok(#call)),
                RetKind::RsgdllResult => quote!(#call.map_err(#rt::Error::from)),
                RetKind::DisplayResult => quote!(#call.map_err(|e| #rt::Error::msg(e))),
            };
            let register = match recv.mutability.is_some() {
                true => quote!(method_mut),
                false => quote!(method),
            };
            Ok(Some(quote! {
                m.#register(#lua_name, |__lua: &#rt::Lua, __this, (#(#names,)*): (#(#params,)*)| #body);
            }))
        })
        .collect::<syn::Result<Vec<_>>>()?
        .into_iter()
        .flatten();
    Ok(quote! {
        #item
        impl #rt::conv::userdata::HasMethods<#ty> for #rt::conv::userdata::MethodsProbe<#ty> {
            fn __rsgdll_methods(&self, m: &mut #rt::conv::UserDataMethods<#ty>) {
                #(#regs)*
            }
        }
    })
}
