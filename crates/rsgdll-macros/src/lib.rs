//! rsgdll proc macros. Generated code refers to the runtime through `::rsgdll::__rt`.

mod asyncfn;
mod derive;
mod function;
mod interface;
mod lua_macro;
mod methods;
mod module;
mod util;

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemFn, ItemImpl, ItemTrait, parse_macro_input};

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

/// `module!(name = "mymod")`: generates `gmod13_open` / `gmod13_close` (spec §8.1).
#[proc_macro]
pub fn module(input: TokenStream) -> TokenStream {
    emit(module::module(input.into()))
}

/// Exports a Rust fn to Lua (spec §8.2).
#[proc_macro_attribute]
pub fn function(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(function::function(attr.into(), parse_macro_input!(item as ItemFn)))
}

#[proc_macro_attribute]
pub fn preopen(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(function::lifecycle("PREOPEN", attr.into(), parse_macro_input!(item as ItemFn)))
}

#[proc_macro_attribute]
pub fn postopen(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(function::lifecycle("POSTOPEN", attr.into(), parse_macro_input!(item as ItemFn)))
}

#[proc_macro_attribute]
pub fn preclose(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(function::lifecycle("PRECLOSE", attr.into(), parse_macro_input!(item as ItemFn)))
}

#[proc_macro_attribute]
pub fn postclose(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(function::lifecycle("POSTCLOSE", attr.into(), parse_macro_input!(item as ItemFn)))
}

/// Methods/metamethods for a `#[derive(LuaUserData)]` type.
#[proc_macro_attribute]
pub fn methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(methods::methods(parse_macro_input!(item as ItemImpl)))
}

/// Declares an engine interface vtable layout (spec §5.2).
#[proc_macro_attribute]
pub fn interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(interface::interface(attr.into(), parse_macro_input!(item as ItemTrait)))
}

/// Inline, compile-time checked GLua (spec §8.3).
#[proc_macro]
pub fn lua(input: TokenStream) -> TokenStream {
    emit(lua_macro::lua(parse_macro_input!(input as lua_macro::LuaInput)))
}
