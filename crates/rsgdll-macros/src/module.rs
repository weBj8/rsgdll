//! `module!(name = "...")` (spec §8.1): emits `gmod13_open` / `gmod13_close`.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{LitStr, parse::Parser};

use crate::util::{err, rt, shadows_global};

pub fn module(input: TokenStream) -> syn::Result<TokenStream> {
    let mut name: Option<LitStr> = None;
    let mut overwrite = false;
    let p = syn::meta::parser(|m| {
        match m.path.get_ident().map(ToString::to_string).as_deref() {
            Some("name") => name = Some(m.value()?.parse()?),
            Some("overwrite") => overwrite = true,
            _ => return Err(m.error("expected `name = \"...\"` or `overwrite`")),
        }
        Ok(())
    });
    p.parse2(input)?;
    let name = name.ok_or_else(|| err(Span::call_site(), "module! needs `name = \"...\"`"))?;
    let n = name.value();
    let valid = !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(err(name.span(), "module name must be a Lua identifier"));
    }
    if !overwrite && shadows_global(&n) {
        return Err(err(
            name.span(),
            format!("module name `{n}` shadows a GMod global; add `overwrite` to replace it"),
        ));
    }
    let rt = rt();
    // `unsafe(no_mangle)` items use mixed-site spans so `forbid(unsafe_code)` in the user
    // crate does not see them as user code (spec §1, gated by a trybuild test).
    let ms = Span::mixed_site();
    let entry = quote_spanned! {ms=>
        #[unsafe(no_mangle)]
        extern "C-unwind" fn gmod13_open(l: *mut #rt::sys::lua_State) -> ::core::ffi::c_int {
            #rt::module::open_entry(l, &__RSGDLL_MODULE)
        }
        #[unsafe(no_mangle)]
        extern "C-unwind" fn gmod13_close(l: *mut #rt::sys::lua_State) -> ::core::ffi::c_int {
            #rt::module::close_entry(l)
        }
    };
    Ok(quote! {
        #[cfg(not(any(feature = "server", feature = "client", feature = "menu")))]
        ::core::compile_error!("declare and enable a realm feature (`server`, `client` or `menu`)");
        #[cfg(all(feature = "server", feature = "client"))]
        ::core::compile_error!("enable exactly one of the `server`/`client` features per build (`menu` may be added to `client`)");

        #[doc(hidden)]
        static __RSGDLL_MODULE: #rt::module::ModuleInfo = #rt::module::ModuleInfo {
            name: #name,
            realms: &[
                #[cfg(feature = "server")] #rt::Realm::Server,
                #[cfg(feature = "client")] #rt::Realm::Client,
                #[cfg(feature = "menu")] #rt::Realm::Menu,
            ],
            gmod: true,
        };

        #[doc(hidden)]
        pub fn __rsgdll_module() -> &'static #rt::module::ModuleInfo {
            &__RSGDLL_MODULE
        }

        #entry
    })
}
