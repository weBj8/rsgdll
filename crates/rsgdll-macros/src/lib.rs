//! rsgdll proc macros. Generated code refers to the runtime through `::rsgdll::__rt`.

mod derive;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

fn emit(r: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    r.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// `FromLua` + `IntoLua` for structs and enums (spec §6.3).
#[proc_macro_derive(LuaValue, attributes(lua))]
pub fn derive_lua_value(input: TokenStream) -> TokenStream {
    emit(derive::lua_value(parse_macro_input!(input as DeriveInput)))
}

/// `UserData` for a Rust type; methods from `#[rsgdll::methods]`.
#[proc_macro_derive(LuaUserData, attributes(lua))]
pub fn derive_lua_userdata(input: TokenStream) -> TokenStream {
    emit(derive::lua_userdata(parse_macro_input!(input as DeriveInput)))
}
