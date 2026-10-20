//! `#[derive(LuaValue)]` and `#[derive(LuaUserData)]` (spec §6.3).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, spanned::Spanned};

/// Container attributes.
#[derive(Default)]
struct Container {
    rename_all: Option<String>,
    tag: Option<String>,
    as_number: bool,
}

/// Field attributes.
#[derive(Default)]
struct FieldAttr {
    rename: Option<String>,
    default: bool,
    skip: bool,
    as_string: bool,
}

fn container(input: &DeriveInput) -> syn::Result<Container> {
    let mut c = Container::default();
    input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("lua"))
        .try_for_each(|a| {
            a.parse_nested_meta(|m| {
                match m.path.get_ident().map(ToString::to_string).as_deref() {
                    Some("rename_all") => {
                        c.rename_all = Some(m.value()?.parse::<LitStr>()?.value())
                    }
                    Some("tag") => c.tag = Some(m.value()?.parse::<LitStr>()?.value()),
                    Some("as_number") => c.as_number = true,
                    _ => return Err(m.error("unknown #[lua] container attribute")),
                }
                Ok(())
            })
        })?;
    Ok(c)
}

fn field_attr(attrs: &[syn::Attribute]) -> syn::Result<FieldAttr> {
    let mut f = FieldAttr::default();
    attrs
        .iter()
        .filter(|a| a.path().is_ident("lua"))
        .try_for_each(|a| {
            a.parse_nested_meta(|m| {
                match m.path.get_ident().map(ToString::to_string).as_deref() {
                    Some("rename") => f.rename = Some(m.value()?.parse::<LitStr>()?.value()),
                    Some("default") => f.default = true,
                    Some("skip") => f.skip = true,
                    Some("as_string") => f.as_string = true,
                    _ => return Err(m.error("unknown #[lua] field attribute")),
                }
                Ok(())
            })
        })?;
    Ok(f)
}

/// Splits a Rust identifier into lowercase words (`steam_id`, `SteamId` -> [steam, id]).
fn words(s: &str) -> Vec<String> {
    s.split('_')
        .flat_map(|part| {
            part.chars().fold(Vec::<String>::new(), |mut acc, c| {
                match (c.is_uppercase(), acc.last()) {
                    (true, _) | (_, None) => acc.push(c.to_lowercase().collect()),
                    (false, Some(_)) => {
                        if let Some(w) = acc.last_mut() {
                            w.push(c)
                        }
                    }
                }
                acc
            })
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn cap(w: &str) -> String {
    let mut c = w.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// serde-compatible `rename_all` rules.
pub fn rename(s: &str, rule: Option<&str>) -> String {
    let ws = words(s);
    match rule {
        None => s.to_owned(),
        Some("PascalCase") => ws.iter().map(|w| cap(w)).collect(),
        Some("camelCase") => ws
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { cap(w) })
            .collect(),
        Some("snake_case") => ws.join("_"),
        Some("SCREAMING_SNAKE_CASE") => ws.join("_").to_uppercase(),
        Some("kebab-case") => ws.join("-"),
        Some("lowercase") => ws.concat(),
        Some("UPPERCASE") => ws.concat().to_uppercase(),
        Some(_) => s.to_owned(),
    }
}

struct NamedField {
    ident: syn::Ident,
    ty: syn::Type,
    key: String,
    attr: FieldAttr,
}

fn named(fields: &syn::FieldsNamed, rule: Option<&str>) -> syn::Result<Vec<NamedField>> {
    fields
        .named
        .iter()
        .map(|f| {
            let attr = field_attr(&f.attrs)?;
            let ident = f
                .ident
                .clone()
                .ok_or_else(|| syn::Error::new(f.span(), "expected named field"))?;
            let key = attr
                .rename
                .clone()
                .unwrap_or_else(|| rename(&ident.to_string(), rule));
            Ok(NamedField {
                ident,
                ty: f.ty.clone(),
                key,
                attr,
            })
        })
        .collect()
}

/// Reads all fields from the table at `__t` and builds `ctor { .. }`.
fn read_fields(rt: &TokenStream, ctor: TokenStream, fields: &[NamedField]) -> TokenStream {
    let inits = fields.iter().map(|f| {
        let (ident, ty, key) = (&f.ident, &f.ty, &f.key);
        let read = match f.attr.as_string {
            true => quote! { #rt::conv::table::field::<#rt::conv::AsString<#ty>>(__lua, __t, #key).map(|v| v.0) },
            false => quote! { #rt::conv::table::field::<#ty>(__lua, __t, #key) },
        };
        match (f.attr.skip, f.attr.default) {
            (true, _) => quote! { #ident: ::core::default::Default::default() },
            (false, true) => quote! {
                #ident: match #rt::conv::table::field_is_nil(__lua, __t, #key)? {
                    true => ::core::default::Default::default(),
                    false => #read?,
                }
            },
            (false, false) => quote! { #ident: #read? },
        }
    });
    quote! { #ctor { #(#inits),* } }
}

/// Stages the fields (bound by name) into the table on top.
fn write_fields(rt: &TokenStream, fields: &[NamedField]) -> TokenStream {
    let n = fields.iter().filter(|f| !f.attr.skip).count();
    let writes = fields.iter().filter(|f| !f.attr.skip).map(|f| {
        let (ident, key) = (&f.ident, &f.key);
        match f.attr.as_string {
            true => {
                quote! { #rt::conv::table::push_field(__s, #key, #rt::conv::AsString(#ident))?; }
            }
            false => quote! { #rt::conv::table::push_field(__s, #key, #ident)?; },
        }
    });
    quote! { __s.table(0, #n); #(#writes)* }
}

pub fn lua_value(input: DeriveInput) -> syn::Result<TokenStream> {
    let rt = quote!(::rsgdll::__rt);
    let c = container(&input)?;
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    let rule = c.rename_all.as_deref();
    let (from, into) = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => {
                let fields = named(f, rule)?;
                let idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();
                let read = read_fields(&rt, quote!(Self), &fields);
                let write = write_fields(&rt, &fields);
                (
                    quote! {
                        let __t = #rt::conv::table::expect_table(__lua, __idx)?;
                        Ok(#read)
                    },
                    quote! {
                        #[allow(unused_variables)]
                        let Self { #(#idents),* } = self;
                        #write
                        Ok(())
                    },
                )
            }
            Fields::Unnamed(f) if f.unnamed.len() == 1 => (
                quote! { #rt::conv::FromLua::from_lua(__lua, __idx).map(Self) },
                quote! { #rt::conv::IntoLua::into_lua(self.0, __s) },
            ),
            _ => {
                return Err(syn::Error::new(
                    input.span(),
                    "LuaValue: use named fields or a newtype",
                ));
            }
        },
        Data::Enum(e) => {
            let unit_only = e.variants.iter().all(|v| matches!(v.fields, Fields::Unit));
            let vname = |v: &syn::Variant| -> syn::Result<String> {
                Ok(field_attr(&v.attrs)?
                    .rename
                    .unwrap_or_else(|| rename(&v.ident.to_string(), rule)))
            };
            match (unit_only, &c.tag, c.as_number) {
                (true, _, true) => {
                    let arms_from = e.variants.iter().map(|v| {
                        let id = &v.ident;
                        quote! { __n if __n == (Self::#id as i64) => Ok(Self::#id), }
                    });
                    (
                        quote! {
                            let __n: i64 = #rt::conv::FromLua::from_lua(__lua, __idx)?;
                            match __n { #(#arms_from)* __n => Err(#rt::ConvError::new(stringify!(#name), format!("number {}", __n))) }
                        },
                        quote! { #rt::conv::IntoLua::into_lua(self as i64, __s) },
                    )
                }
                (true, _, false) => {
                    let names: Vec<String> =
                        e.variants.iter().map(vname).collect::<syn::Result<_>>()?;
                    let ids: Vec<_> = e.variants.iter().map(|v| &v.ident).collect();
                    let expected = names.join(" | ");
                    (
                        quote! {
                            let __v: String = #rt::conv::FromLua::from_lua(__lua, __idx)?;
                            match __v.as_str() {
                                #(#names => Ok(Self::#ids),)*
                                __o => Err(#rt::ConvError::new(#expected, format!("\"{}\"", __o))),
                            }
                        },
                        quote! { #rt::conv::IntoLua::into_lua(match self { #(Self::#ids => #names,)* }, __s) },
                    )
                }
                (false, Some(tag), _) => {
                    let mut arms_from = Vec::new();
                    let mut arms_into = Vec::new();
                    e.variants.iter().try_for_each(|v| -> syn::Result<()> {
                        let id = &v.ident;
                        let key = vname(v)?;
                        let fields = match &v.fields {
                            Fields::Named(f) => named(f, None)?,
                            Fields::Unit => Vec::new(),
                            Fields::Unnamed(_) => {
                                return Err(syn::Error::new(
                                    v.span(),
                                    "tagged enums need named or unit variants",
                                ));
                            }
                        };
                        let idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();
                        let read = read_fields(&rt, quote!(Self::#id), &fields);
                        let write = write_fields(&rt, &fields);
                        arms_from.push(quote! { #key => Ok(#read), });
                        arms_into.push(quote! {
                            Self::#id { #(#idents),* } => {
                                #write
                                #rt::conv::table::push_field(__s, #tag, #key)?;
                            }
                        });
                        Ok(())
                    })?;
                    (
                        quote! {
                            let __t = #rt::conv::table::expect_table(__lua, __idx)?;
                            let __k: String = #rt::conv::table::field(__lua, __t, #tag)?;
                            match __k.as_str() {
                                #(#arms_from)*
                                __o => Err(#rt::ConvError::new(concat!("variant of ", stringify!(#name)), format!("\"{}\"", __o)).at(#tag)),
                            }
                        },
                        quote! { match self { #(#arms_into)* } Ok(()) },
                    )
                }
                (false, None, _) => {
                    return Err(syn::Error::new(
                        input.span(),
                        "LuaValue: enums with data need #[lua(tag = \"...\")]",
                    ));
                }
            }
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "LuaValue: unions are not supported",
            ));
        }
    };
    Ok(quote! {
        impl #ig #rt::conv::FromLua for #name #tg #wc {
            fn from_lua(__lua: &#rt::Lua, __idx: ::core::ffi::c_int) -> ::core::result::Result<Self, #rt::ConvError> { #from }
        }
        impl #ig #rt::conv::IntoLua for #name #tg #wc {
            fn into_lua(self, __s: &mut #rt::conv::Stage<'_>) -> ::core::result::Result<(), #rt::ConvError> { #into }
        }
    })
}

/// `#[derive(LuaUserData)]`: implements `UserData`. Methods registered by an optional
/// `#[rsgdll::methods]` impl are found by autoref lookup (`HasMethods` beats `NoMethods`).
pub fn lua_userdata(input: DeriveInput) -> syn::Result<TokenStream> {
    let rt = quote!(::rsgdll::__rt);
    let name = &input.ident;
    let lit = name.to_string();
    Ok(quote! {
        impl #rt::conv::UserData for #name {
            const NAME: &'static str = #lit;
            fn methods(m: &mut #rt::conv::UserDataMethods<Self>) {
                #[allow(unused_imports)]
                use #rt::conv::userdata::{HasMethods as _, NoMethods as _};
                (&#rt::conv::userdata::MethodsProbe::<#name>(::core::marker::PhantomData)).__rsgdll_methods(m)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::rename;

    #[test]
    fn rename_rules() {
        assert_eq!(rename("steam_id", Some("PascalCase")), "SteamId");
        assert_eq!(rename("steam_id", Some("camelCase")), "steamId");
        assert_eq!(rename("MyVariant", Some("snake_case")), "my_variant");
        assert_eq!(rename("retries", None), "retries");
    }
}
