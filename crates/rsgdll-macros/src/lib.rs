//! Developer-facing procedural macros.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, ItemFn, Pat, PathArguments, ReturnType, Type, parse_macro_input,
};

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
    let name = &function.sig.ident;
    quote! {
        #function

        #[doc(hidden)]
        unsafe extern "C" fn __rsgdll_module_initialize(
            registrations: *mut ::rsgdll::__private::module::RawRegistration,
            capacity: u32,
            output_count: *mut u32,
        ) -> u8 {
            // SAFETY: C++ module entrypoint supplies its live fixed registration
            // array and writable count for this call.
            unsafe {
                ::rsgdll::__private::module::initialize_module(
                    registrations,
                    capacity,
                    output_count,
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
/// `Result<T, E>` is recognized syntactically. A return type alias hiding
/// `Result` is therefore treated as a plain return value in this initial API.
#[proc_macro_attribute]
pub fn function(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return compile_error("`function` does not accept arguments");
    }
    let function = parse_macro_input!(item as ItemFn);
    match expand_function(function) {
        Ok(tokens) => tokens.into(),
        Err(message) => compile_error(message),
    }
}

fn expand_function(mut function: ItemFn) -> Result<proc_macro2::TokenStream, &'static str> {
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
    let mut argument_names = Vec::new();
    for (offset, argument) in function.sig.inputs.iter().enumerate() {
        let FnArg::Typed(argument) = argument else {
            return Err("`function` does not support method receivers");
        };
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err("`function` parameters must use simple identifier patterns");
        };
        let name = &pattern.ident;
        let ty = &argument.ty;
        let index = i32::try_from(offset + 1).map_err(|_| "`function` has too many parameters")?;
        arguments.push(quote! {
            let #name: #ty = frame
                .get(#index)
                .map_err(|error| -> ::rsgdll::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                })?;
        });
        argument_names.push(name.clone());
    }

    let return_type = match &function.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
    };
    let (result_type, is_result) = return_type
        .and_then(syntactic_result_ok_type)
        .map_or((return_type, false), |ty| (Some(ty), true));
    let call = quote! { #implementation_name(#(#argument_names),*) };
    let evaluate = if is_result {
        quote! {
            let output = #call.map_err(
                |error| -> ::rsgdll::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
        }
    } else if is_unit(result_type) {
        quote! { #call; }
    } else {
        quote! { let output = #call; }
    };
    let stage = stage_returns(result_type);

    function.sig.ident = implementation_name;
    function.vis = syn::Visibility::Inherited;
    let configuration_attributes: Vec<_> = function
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .collect();

    Ok(quote! {
        #function

        #(#configuration_attributes)*
        #[allow(non_upper_case_globals)]
        #visibility const #descriptor_name: ::rsgdll::__private::module::Function =
            ::rsgdll::__private::module::Function::new(
                concat!(module_path!(), "::", stringify!(#descriptor_name)),
                #callback_name,
            );

        #(#configuration_attributes)*
        fn #callback_name(
            frame: &mut ::rsgdll::__private::lua::StackFrame<'_, '_>,
            returns: &mut ::rsgdll::__private::module::ReturnWriter<'_>,
        ) -> ::std::result::Result<(), ::rsgdll::__private::module::BoxError> {
            #(#arguments)*
            #evaluate
            #stage
            ::std::result::Result::Ok(())
        }
    })
}

fn syntactic_result_ok_type(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let ok = types.next()?;
    types.next()?;
    types.next().is_none().then_some(ok)
}

fn is_unit(ty: Option<&Type>) -> bool {
    match ty {
        None => true,
        Some(Type::Tuple(tuple)) => tuple.elems.is_empty(),
        Some(_) => false,
    }
}

fn stage_returns(ty: Option<&Type>) -> proc_macro2::TokenStream {
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
                        |error| -> ::rsgdll::__private::module::BoxError {
                            ::std::boxed::Box::new(error)
                        },
                    )?;
                )*
            }
        }
        Some(_) => quote! {
            returns.push(output).map_err(
                |error| -> ::rsgdll::__private::module::BoxError {
                    ::std::boxed::Box::new(error)
                },
            )?;
        },
    }
}

fn compile_error(message: &str) -> TokenStream {
    syn::Error::new(proc_macro2::Span::call_site(), message)
        .into_compile_error()
        .into()
}
