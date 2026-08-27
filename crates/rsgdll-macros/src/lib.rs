//! Developer-facing procedural macros.

mod signature;

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use signature::{
    is_main_thread_context, is_stack_frame_context, is_unit, syntactic_result_ok_type,
};
use syn::{FnArg, ItemFn, Pat, ReturnType, Type, parse_macro_input};

/// Marks the one function that registers a binary module's Lua API.
#[proc_macro_attribute]
pub fn module(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return compile_error("`module` does not accept arguments");
    }
    let function = parse_macro_input!(item as ItemFn);
    if function.sig.asyncness.is_some()
        || function.sig.constness.is_some()
        || function.sig.unsafety.is_some()
        || function.sig.abi.is_some()
        || !function.sig.generics.params.is_empty()
        || function.sig.inputs.len() != 1
        || !matches!(function.sig.output, ReturnType::Default)
    {
        return compile_error(
            "`module` requires `fn name(module: &mut ModuleBuilder)` with no return value",
        );
    }
    let facade = match facade_path() {
        Ok(facade) => facade,
        Err(error) => return error.into_compile_error().into(),
    };
    let name = &function.sig.ident;
    quote! {
        #function

        #[doc(hidden)]
        unsafe extern "C" fn __rsgdll_module_initialize(
            registrations: *mut #facade::__private::module::RawRegistration,
            capacity: u32,
            output_count: *mut u32,
            output_name: *mut *const u8,
            output_name_length: *mut u32,
            output_abi_layout: *mut *const #facade::__private::module::AbiLayout,
            error_buffer: *mut ::std::ffi::c_char,
            error_capacity: u32,
        ) -> u8 {
            // SAFETY: [Category 3 — dangling pointers] C++ supplies its live
            // fixed registration array and writable POD outputs for this call.
            unsafe {
                #facade::__private::module::initialize_module(
                    registrations,
                    capacity,
                    output_count,
                    (output_name, output_name_length, output_abi_layout),
                    (error_buffer, error_capacity),
                    module_path!(),
                    #name,
                )
            }
        }

        #[doc(hidden)]
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn gmod13_open(
            _state: *mut ::std::ffi::c_void,
        ) -> ::std::ffi::c_int {
            ::core::arch::naked_asm!(
                "lea rsi, [rip + {initializer}]",
                "jmp rsgdll_bridge_gmod13_open",
                initializer = sym __rsgdll_module_initialize,
            );
        }

        #[doc(hidden)]
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn gmod13_close(
            _state: *mut ::std::ffi::c_void,
        ) -> ::std::ffi::c_int {
            ::core::arch::naked_asm!("jmp rsgdll_bridge_gmod13_close");
        }
    }
    .into()
}

/// Converts a Rust function into a descriptor accepted by `ModuleBuilder`.
///
/// Pass `serde` to deserialize each Lua parameter and serialize the return
/// value through the facade's optional `serde` feature.
///
/// `Result<T, E>` is recognized syntactically. A return type alias hiding
/// `Result` is therefore treated as a plain return value in this initial API.
#[proc_macro_attribute]
pub fn function(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let serde = if attribute.is_empty() {
        false
    } else {
        let mode = parse_macro_input!(attribute as syn::Ident);
        if mode != "serde" {
            return compile_error("`function` accepts only the optional `serde` argument");
        }
        true
    };
    let function = parse_macro_input!(item as ItemFn);
    let facade = match facade_path() {
        Ok(facade) => facade,
        Err(error) => return error.into_compile_error().into(),
    };
    match expand_function(function, &facade, serde) {
        Ok(tokens) => tokens.into(),
        Err(message) => compile_error(message),
    }
}

fn expand_function(
    mut function: ItemFn,
    facade: &proc_macro2::TokenStream,
    serde: bool,
) -> Result<proc_macro2::TokenStream, &'static str> {
    if function.sig.asyncness.is_some()
        || function.sig.constness.is_some()
        || function.sig.unsafety.is_some()
        || function.sig.abi.is_some()
        || function.sig.variadic.is_some()
        || !function.sig.generics.params.is_empty()
    {
        return Err("`function` supports only non-generic safe Rust functions");
    }

    let descriptor_name = function.sig.ident.clone();
    let implementation_name = format_ident!("__rsgdll_impl_{}", descriptor_name);
    let callback_name = format_ident!("__rsgdll_callback_{}", descriptor_name);
    let visibility = function.vis.clone();
    let mut arguments = Vec::new();
    let mut call_arguments = Vec::new();
    let mut has_main_thread = false;
    let mut has_stack_frame = false;
    let mut lua_index = 0_i32;
    for argument in &function.sig.inputs {
        let FnArg::Typed(argument) = argument else {
            return Err("`function` does not support method receivers");
        };
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err("`function` parameters must use simple identifier patterns");
        };
        let name = &pattern.ident;
        let ty = &argument.ty;
        if is_stack_frame_context(ty) {
            if has_stack_frame || lua_index != 0 {
                return Err("`&mut StackFrame` may appear once before Lua value parameters");
            }
            has_stack_frame = true;
            call_arguments.push(quote! { frame });
            continue;
        }
        if is_main_thread_context(ty) {
            if has_main_thread || lua_index != 0 {
                return Err("`&mut MainThread` may appear once before Lua value parameters");
            }
            has_main_thread = true;
            call_arguments.push(quote! { &mut main_thread });
            continue;
        }
        lua_index = lua_index
            .checked_add(1)
            .ok_or("`function` has too many parameters")?;
        let index = lua_index;
        arguments.push(if serde {
            quote! {
                let #name: #ty = #facade::lua::serde::from_lua(frame, #index)
                    .map_err(|error| -> #facade::__private::module::BoxError {
                        ::std::boxed::Box::new(error)
                    })?;
            }
        } else {
            quote! {
                let #name: #ty = frame
                    .get(#index)
                    .map_err(|error| -> #facade::__private::module::BoxError {
                        ::std::boxed::Box::new(error)
                    })?;
            }
        });
        call_arguments.push(quote! { #name });
    }

    let return_type = match &function.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
    };
    let (result_type, is_result) = return_type
        .and_then(syntactic_result_ok_type)
        .map_or((return_type, false), |ty| (Some(ty), true));
    let call = quote! { #implementation_name(#(#call_arguments),*) };
    let evaluate = if is_result {
        quote! {
            let output = #call.map_err(
                |error| -> #facade::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
        }
    } else if is_unit(result_type) {
        quote! { #call; }
    } else {
        quote! { let output = #call; }
    };
    let stage = stage_returns(result_type, facade, serde);

    function.sig.ident = implementation_name;
    function.vis = syn::Visibility::Inherited;
    let configuration_attributes: Vec<_> = function
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .collect();
    let main_thread = has_main_thread.then(|| {
        quote! {
            // SAFETY: generated glue runs only inside the framework dispatcher
            // while `frame` proves ownership of the GMod main-thread callback.
            let mut main_thread = unsafe {
                #facade::__private::runtime::main_thread_from_callback(frame)
            };
        }
    });

    Ok(quote! {
        #function

        #(#configuration_attributes)*
        #[allow(non_upper_case_globals)]
        #visibility const #descriptor_name: #facade::__private::module::Function =
            #facade::__private::module::Function::new(
                concat!(module_path!(), "::", stringify!(#descriptor_name)),
                #callback_name,
            );

        #(#configuration_attributes)*
        fn #callback_name(
            frame: &mut #facade::__private::lua::StackFrame<'_, '_>,
            returns: &mut #facade::__private::module::ReturnWriter<'_>,
        ) -> ::std::result::Result<(), #facade::__private::module::BoxError> {
            #main_thread
            #(#arguments)*
            #evaluate
            #stage
            ::std::result::Result::Ok(())
        }
    })
}

fn stage_returns(
    ty: Option<&Type>,
    facade: &proc_macro2::TokenStream,
    serde: bool,
) -> proc_macro2::TokenStream {
    if serde && ty.is_some_and(|ty| !is_unit(Some(ty))) {
        return quote! {
            #facade::lua::serde::to_lua(frame, &output).map_err(
                |error| -> #facade::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
            returns.push(#facade::module::LuaStackValues::new(1)).map_err(
                |error| -> #facade::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
        };
    }
    match ty {
        None => quote! {},
        Some(Type::Tuple(tuple)) if tuple.elems.is_empty() => quote! {},
        Some(Type::Tuple(tuple)) => {
            let values: Vec<_> = (0..tuple.elems.len())
                .map(|index| format_ident!("__rsgdll_return_{index}"))
                .collect();
            quote! {
                let (#(#values,)*) = output;
                #(
                    returns.push(#values).map_err(
                        |error| -> #facade::__private::module::BoxError {
                            ::std::boxed::Box::new(error)
                        },
                    )?;
                )*
            }
        }
        Some(_) => quote! {
            returns.push(output).map_err(
                |error| -> #facade::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
        },
    }
}

fn facade_path() -> Result<proc_macro2::TokenStream, syn::Error> {
    match crate_name("rsgdll") {
        Ok(FoundCrate::Itself) => Ok(quote!(crate)),
        Ok(FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            Ok(quote!(::#ident))
        }
        Err(error) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to locate the `rsgdll` facade dependency: {error}"),
        )),
    }
}

fn compile_error(message: &str) -> TokenStream {
    syn::Error::new(proc_macro2::Span::call_site(), message)
        .into_compile_error()
        .into()
}
