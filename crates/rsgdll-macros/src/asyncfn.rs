//! Async `#[rsgdll::function]` (spec §10.2). Implemented with the executor (t7).

use proc_macro2::TokenStream;
use syn::{ItemFn, spanned::Spanned};

use crate::function::FnAttr;

pub fn async_function(_a: FnAttr, item: ItemFn) -> syn::Result<TokenStream> {
    Err(syn::Error::new(item.sig.asyncness.span(), "async exports are not implemented yet"))
}
