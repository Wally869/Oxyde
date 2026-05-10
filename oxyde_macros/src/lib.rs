use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, ItemStruct, Type};

/// Load a compiled contract's artifacts (bytecode, ABI, typed wrapper) from
/// build script output. Expands to a module containing the generated code
/// and re-exports all symbols.
///
/// ```ignore
/// oxyde_sdk::load_contract!(ERC20);
///
/// let token = ERC20::deploy(&mut env, ...)?;
/// ```
#[proc_macro]
pub fn load_contract(input: TokenStream) -> TokenStream {
    let ident = parse_macro_input!(input as syn::Ident);
    let name = ident.to_string();
    let mod_name = format_ident!("__{}_artifacts", name.to_lowercase());
    let file_path = format!("/{}.rs", name);

    TokenStream::from(quote! {
        mod #mod_name {
            include!(concat!(env!("OUT_DIR"), #file_path));
        }
        use #mod_name::*;
    })
}

/// Marks a struct as an Oxyde contract. Validates that all fields are
/// `StorageItem<T>` or `StorageMap<K, V>`.
#[proc_macro_attribute]
pub fn contract(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input_struct = parse_macro_input!(input as ItemStruct);

    if let syn::Fields::Named(ref fields) = input_struct.fields {
        for field in &fields.named {
            let field_name = field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_default();

            if !is_storage_type(&field.ty) {
                let msg = format!(
                    "field '{}': all contract fields must be StorageItem<T>, StorageMap<K, V>, or StorageDoubleMap<K1, K2, V>",
                    field_name
                );
                return syn::Error::new_spanned(&field.ty, msg)
                    .to_compile_error()
                    .into();
            }
        }
    } else {
        return syn::Error::new_spanned(
            &input_struct,
            "contract struct must have named fields",
        )
        .to_compile_error()
        .into();
    }

    TokenStream::from(quote! { #input_struct })
}

fn is_storage_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let name = segment.ident.to_string();
            return name == "StorageItem" || name == "StorageMap" || name == "StorageDoubleMap";
        }
    }
    false
}

/// Marks a contract struct as extending a parent contract. The attribute is a
/// passthrough marker consumed by the build-script codegen, not by the Rust compiler.
#[proc_macro_attribute]
pub fn extends(_attr: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Marks an impl block as containing contract entrypoints. Validates method signatures.
#[proc_macro_attribute]
pub fn entrypoints(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input_impl = parse_macro_input!(input as ItemImpl);

    for item in &input_impl.items {
        if let ImplItem::Fn(method) = item {
            if !matches!(method.vis, syn::Visibility::Public(_)) {
                continue;
            }

            let method_name = method.sig.ident.to_string();

            if method_name == "constructor" {
                let has_receiver = method
                    .sig
                    .inputs
                    .iter()
                    .any(|arg| matches!(arg, FnArg::Receiver(_)));
                if has_receiver {
                    return syn::Error::new_spanned(
                        &method.sig,
                        "constructor must not take self — it should return Self",
                    )
                    .to_compile_error()
                    .into();
                }
                continue;
            }

            let has_receiver = method
                .sig
                .inputs
                .iter()
                .any(|arg| matches!(arg, FnArg::Receiver(_)));
            if !has_receiver {
                return syn::Error::new_spanned(
                    &method.sig,
                    format!(
                        "method '{}': contract methods must take &self or &mut self",
                        method_name
                    ),
                )
                .to_compile_error()
                .into();
            }
        }
    }

    TokenStream::from(quote! { #input_impl })
}
