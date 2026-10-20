//! `#[rsgdll::interface("VEngineServer023")] unsafe trait EngineServer { fn ...; }`
//! (spec §5.2). Each trait method becomes one vtable slot in declaration order (Itanium/
//! GCC order; the user's slot list is the unchecked claim, hence `unsafe trait`).

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemTrait, LitStr, TraitItem, spanned::Spanned};

use crate::util::{err, rt};

pub fn interface(attr: TokenStream, item: ItemTrait) -> syn::Result<TokenStream> {
    let name: LitStr = syn::parse2(attr)?;
    if item.unsafety.is_none() {
        return Err(err(item.trait_token.span(), "#[rsgdll::interface] traits must be `unsafe trait` (the slot layout is unchecked)"));
    }
    let rt = rt();
    let tr = &item.ident;
    let vis = &item.vis;
    let vt = format_ident!("{}VTable", tr);
    let obj = format_ident!("{}Object", tr);
    let methods: Vec<&syn::TraitItemFn> = item
        .items
        .iter()
        .filter_map(|i| match i {
            TraitItem::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    let slots = methods
        .iter()
        .map(|f| -> syn::Result<TokenStream> {
            let id = &f.sig.ident;
            if !matches!(f.sig.inputs.first(), Some(FnArg::Receiver(_))) {
                return Err(err(f.sig.span(), "interface methods take `&self`"));
            }
            let args: Vec<&syn::Type> = f.sig.inputs.iter().skip(1).filter_map(|a| match a {
                FnArg::Typed(t) => Some(&*t.ty),
                _ => None,
            }).collect();
            let ret = &f.sig.output;
            Ok(match f.default.is_some() || id.to_string().starts_with("_pad") {
                true => quote!(pub #id: usize,),
                false => quote!(pub #id: #rt::sys::vfn!((*mut ::core::ffi::c_void #(, #args)*) #ret),),
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let impls = methods
        .iter()
        .filter(|f| f.default.is_none() && !f.sig.ident.to_string().starts_with("_pad"))
        .map(|f| {
            let id = &f.sig.ident;
            let sig = &f.sig;
            let args: Vec<TokenStream> = f.sig.inputs.iter().skip(1).filter_map(|a| match a {
                FnArg::Typed(t) => {
                    let p = &t.pat;
                    Some(quote!(#p))
                }
                _ => None,
            }).collect();
            quote! {
                #sig {
                    // SAFETY: guaranteed by the `unsafe impl` of the trait for this object.
                    unsafe { ((*self.vtable).#id)((self as *const Self).cast_mut().cast() #(, #args)*) }
                }
            }
        });
    let sigs = methods.iter().map(|f| {
        let sig = &f.sig;
        quote!(#sig;)
    });
    Ok(quote! {
        #vis unsafe trait #tr { #(#sigs)* }

        #[repr(C)]
        #[allow(non_snake_case)]
        #vis struct #vt { #(#slots)* }

        /// The C++ object (`vtable` pointer first).
        #[repr(C)]
        #vis struct #obj { pub vtable: *const #vt }

        unsafe impl #tr for #obj { #(#impls)* }

        impl #obj {
            pub const NAME: &'static str = #name;

            /// `CreateInterface(NAME)` from the engine library `module`.
            pub fn create(module: &str) -> ::core::option::Option<&'static #obj> {
                // SAFETY: the `unsafe trait` declaration is the layout claim.
                unsafe { #rt::interface::create_interface::<#obj>(module, #name) }
            }
        }
    })
}
