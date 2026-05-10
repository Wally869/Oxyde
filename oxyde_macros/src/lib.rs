use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, ItemStruct, Type};

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
                    "field '{}': all contract fields must be StorageItem<T> or StorageMap<K, V>",
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
            return name == "StorageItem" || name == "StorageMap";
        }
    }
    false
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
