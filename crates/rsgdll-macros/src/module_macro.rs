use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, ReturnType, parse_macro_input};

use super::{compile_error, facade_path};

pub(super) fn expand(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let close = if attribute.is_empty() {
        None
    } else {
        let option = parse_macro_input!(attribute as syn::MetaNameValue);
        if !option.path.is_ident("close") {
            return compile_error("`module` accepts only the optional `close = path` argument");
        }
        let syn::Expr::Path(close) = option.value else {
            return compile_error("`module` requires `close` to name a safe `fn()`");
        };
        if close.qself.is_some() {
            return compile_error("`module` requires `close` to name a safe `fn()`");
        }
        Some(close.path)
    };
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
    let close_entry = close.map_or_else(
        || {
            quote! {
                #[doc(hidden)]
                #[unsafe(naked)]
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn gmod13_close(
                    _state: *mut ::std::ffi::c_void,
                ) -> ::std::ffi::c_int {
                    ::core::arch::naked_asm!("jmp rsgdll_bridge_gmod13_close");
                }
            }
        },
        |close| {
            quote! {
                #[doc(hidden)]
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn gmod13_close(
                    _state: *mut ::std::ffi::c_void,
                ) -> ::std::ffi::c_int {
                    let close: fn() = #close;
                    let _ = ::std::panic::catch_unwind(close);
                    0
                }
            }
        },
    );
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

        #close_entry
    }
    .into()
}
