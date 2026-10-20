//! Shared macro helpers.

use proc_macro2::{Span, TokenStream};
use quote::quote;

pub fn rt() -> TokenStream {
    quote!(::rsgdll::__rt)
}

/// `#[distributed_slice(SLICE)]` attributes for a static element.
pub fn slice_attrs(slice: &str) -> TokenStream {
    let rt = rt();
    let slice: TokenStream = slice.parse().unwrap_or_default();
    quote! {
        #[#rt::linkme::distributed_slice(#rt::module::#slice)]
        #[linkme(crate = #rt::linkme)]
    }
}

/// `snake_case` -> `PascalCase`.
pub fn pascal(s: &str) -> String {
    s.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect()
}

pub fn err(span: Span, msg: impl std::fmt::Display) -> syn::Error {
    syn::Error::new(span, msg)
}

/// A compile-time warning: calls a `#[deprecated]` fn whose note is the message.
pub fn warning(span: Span, msg: &str) -> TokenStream {
    let note = format!("rsgdll: {msg}");
    quote::quote_spanned! {span=>
        {
            #[deprecated(note = #note)]
            #[allow(non_snake_case)]
            const fn rsgdll_warning() {}
            rsgdll_warning();
        }
    }
}

/// Whether `ty` is `&Lua` / `&rsgdll::Lua`.
pub fn is_lua_ref(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(r) => matches!(&*r.elem, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Lua")),
        _ => false,
    }
}

/// Last path segment ident of a type (`Variadic`, `Option`, ...).
pub fn last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// Parses `realm = "server"` values.
pub fn realm_tokens(span: Span, r: &str) -> syn::Result<TokenStream> {
    let rt = rt();
    match r {
        "server" => Ok(quote!(#rt::Realm::Server)),
        "client" => Ok(quote!(#rt::Realm::Client)),
        "menu" => Ok(quote!(#rt::Realm::Menu)),
        _ => Err(err(span, "realm must be \"server\", \"client\" or \"menu\"")),
    }
}

/// cfg-gated realm violation for an item available only in `realm` (spec §9.2 check 5).
pub fn realm_guard(span: Span, realm: &str, what: &str) -> TokenStream {
    let msg = format!("{what} is {realm}-only and this crate does not build the {realm} realm");
    match realm {
        "server" => quote::quote_spanned! {span=> #[cfg(all(feature = "client", not(feature = "server")))] ::core::compile_error!(#msg); },
        "client" => quote::quote_spanned! {span=> #[cfg(all(feature = "server", not(feature = "client")))] ::core::compile_error!(#msg); },
        "menu" => quote::quote_spanned! {span=> #[cfg(not(feature = "menu"))] ::core::compile_error!(#msg); },
        _ => TokenStream::new(),
    }
}

/// Name collides with a known GMod global/library (spec §8.2).
pub fn shadows_global(name: &str) -> bool {
    let root = name.split('.').next().unwrap_or(name);
    rsgdll_apidb::db().is_global(root)
}
