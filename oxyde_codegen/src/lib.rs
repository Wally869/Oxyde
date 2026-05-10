pub mod abi;
pub mod artifact;
pub mod bytecode;
pub mod compiler;
pub mod layout;
pub mod opcodes;
pub mod types;

use std::collections::HashMap;

/// Parse a Rust source string containing a single contract struct + impl block
/// and compile it into a [`artifact::ContractArtifact`].
pub fn compile_contract_source(source: &str) -> Result<artifact::ContractArtifact, String> {
    let file = syn::parse_file(source).map_err(|e| format!("parse error: {e}"))?;

    let contract_struct = file
        .items
        .iter()
        .find_map(|item| {
            if let syn::Item::Struct(s) = item {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or("no struct found")?;

    let struct_name = contract_struct.ident.to_string();
    let contract_impl = file
        .items
        .iter()
        .find_map(|item| {
            if let syn::Item::Impl(imp) = item {
                if let syn::Type::Path(path) = &*imp.self_ty {
                    if path.path.segments.last().map(|s| s.ident.to_string())
                        == Some(struct_name.clone())
                    {
                        return Some(imp.clone());
                    }
                }
            }
            None
        })
        .ok_or("no impl block found")?;

    let compiled = compiler::compile(&contract_struct, &contract_impl)?;
    Ok(artifact::ContractArtifact::from_compiled(compiled))
}

/// Compile a contract source with inheritance resolution.
/// `available_sources` maps source file content by file stem (e.g. "erc20_base" -> source).
/// If the struct carries `#[oxyde_macros::extends(ParentName)]` (or `#[extends(ParentName)]`),
/// the parent struct is resolved from `available_sources`, and its fields and methods are
/// merged into the child before compilation.
pub fn compile_contract_with_deps(
    source: &str,
    available_sources: &HashMap<String, String>,
) -> Result<artifact::ContractArtifact, String> {
    let file = syn::parse_file(source).map_err(|e| format!("parse error: {e}"))?;

    let mut contract_struct = find_struct(&file)?;
    let struct_name = contract_struct.ident.to_string();
    let mut contract_impl = find_impl(&file, &struct_name)?;

    if let Some(parent_name) = extract_extends_attr(&contract_struct) {
        let parent_source = resolve_parent(&parent_name, available_sources)?;
        let parent_file =
            syn::parse_file(&parent_source).map_err(|e| format!("parse error in parent: {e}"))?;
        let parent_struct = find_struct(&parent_file)?;
        let parent_impl = find_impl(&parent_file, &parent_struct.ident.to_string())?;

        merge_parent(&mut contract_struct, &mut contract_impl, &parent_struct, &parent_impl)?;

        // Remove extends attribute so the downstream compiler doesn't see it.
        contract_struct.attrs.retain(|a| !is_extends_attr(a));
    }

    // Build external ABIs from all sibling contracts
    let external_abis = build_external_abis(available_sources, &struct_name)?;

    let compiled = compiler::compile_with_externals(&contract_struct, &contract_impl, &external_abis)?;
    Ok(artifact::ContractArtifact::from_compiled(compiled))
}

fn build_external_abis(
    sources: &HashMap<String, String>,
    exclude_name: &str,
) -> Result<HashMap<String, compiler::ExternalContractAbi>, String> {
    let mut abis = HashMap::new();
    for source in sources.values() {
        if let Ok(file) = syn::parse_file(source) {
            if let Ok(s) = find_struct(&file) {
                let name = s.ident.to_string();
                if name == exclude_name {
                    continue;
                }
                if let Ok(imp) = find_impl(&file, &name) {
                    // Resolve inheritance so child contracts expose parent methods
                    let mut merged_struct = s.clone();
                    let mut merged_impl = imp.clone();
                    if let Some(parent_name) = extract_extends_attr(&merged_struct) {
                        if let Ok(parent_source) = resolve_parent(&parent_name, sources) {
                            if let Ok(parent_file) = syn::parse_file(&parent_source) {
                                if let Ok(parent_struct) = find_struct(&parent_file) {
                                    if let Ok(parent_impl) = find_impl(&parent_file, &parent_struct.ident.to_string()) {
                                        let _ = merge_parent(&mut merged_struct, &mut merged_impl, &parent_struct, &parent_impl);
                                    }
                                }
                            }
                        }
                    }

                    let mut functions = Vec::new();
                    for item in &merged_impl.items {
                        if let syn::ImplItem::Fn(method) = item {
                            if !matches!(method.vis, syn::Visibility::Public(_)) {
                                continue;
                            }
                            let method_name = method.sig.ident.to_string();
                            if method_name == "constructor" {
                                continue;
                            }
                            let method_args = compiler::extract_typed_args(method);
                            let output_type = match &method.sig.output {
                                syn::ReturnType::Default => None,
                                syn::ReturnType::Type(_, ty) => Some(abi::syn_type_to_string(ty)),
                            };
                            let mutability = compiler::infer_mutability(method);
                            if let Ok(fabi) = abi::FunctionAbi::new(
                                &method_name,
                                &method_args,
                                output_type.as_deref(),
                                mutability,
                            ) {
                                functions.push(fabi);
                            }
                        }
                    }
                    abis.insert(name, compiler::ExternalContractAbi { functions });
                }
            }
        }
    }
    Ok(abis)
}

// ---- Helpers ----

fn find_struct(file: &syn::File) -> Result<syn::ItemStruct, String> {
    file.items
        .iter()
        .find_map(|item| {
            if let syn::Item::Struct(s) = item {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| "no struct found in source".to_string())
}

fn find_impl(file: &syn::File, struct_name: &str) -> Result<syn::ItemImpl, String> {
    file.items
        .iter()
        .find_map(|item| {
            if let syn::Item::Impl(imp) = item {
                if let syn::Type::Path(path) = &*imp.self_ty {
                    if path.path.segments.last().map(|s| s.ident.to_string())
                        == Some(struct_name.to_string())
                    {
                        return Some(imp.clone());
                    }
                }
            }
            None
        })
        .ok_or_else(|| format!("no impl block found for '{struct_name}'"))
}

fn extract_extends_attr(s: &syn::ItemStruct) -> Option<String> {
    s.attrs.iter().find_map(|attr| {
        if !is_extends_attr(attr) {
            return None;
        }
        // Parse the token tree inside the attribute to get the parent name.
        // Attribute looks like: #[oxyde_macros::extends(ParentName)]
        let meta_list = attr.meta.require_list().ok()?;
        let ident: syn::Ident = syn::parse2(meta_list.tokens.clone()).ok()?;
        Some(ident.to_string())
    })
}

fn is_extends_attr(attr: &syn::Attribute) -> bool {
    let path = attr.path();
    // Match both `extends(...)` and `oxyde_macros::extends(...)`
    let segments: Vec<_> = path.segments.iter().collect();
    match segments.as_slice() {
        [only] => only.ident == "extends",
        [_prefix, last] => last.ident == "extends",
        _ => false,
    }
}

fn resolve_parent(
    name: &str,
    sources: &HashMap<String, String>,
) -> Result<String, String> {
    for source in sources.values() {
        if let Ok(file) = syn::parse_file(source) {
            if let Ok(s) = find_struct(&file) {
                if s.ident == name {
                    return Ok(source.clone());
                }
            }
        }
    }
    Err(format!("could not find parent contract '{name}' in available sources"))
}

fn merge_parent(
    child_struct: &mut syn::ItemStruct,
    child_impl: &mut syn::ItemImpl,
    parent_struct: &syn::ItemStruct,
    parent_impl: &syn::ItemImpl,
) -> Result<(), String> {
    // --- Merge fields: prepend parent fields before child fields ---
    let child_named = match &mut child_struct.fields {
        syn::Fields::Named(f) => f,
        _ => return Err("child struct must have named fields".to_string()),
    };
    let parent_named = match &parent_struct.fields {
        syn::Fields::Named(f) => f,
        _ => return Err("parent struct must have named fields".to_string()),
    };

    // Validate: no field name collisions
    for parent_field in &parent_named.named {
        let parent_field_name = parent_field
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_default();
        for child_field in &child_named.named {
            let child_field_name = child_field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_default();
            if parent_field_name == child_field_name {
                return Err(format!(
                    "field name collision: both parent and child define '{parent_field_name}'"
                ));
            }
        }
    }

    // Prepend parent fields
    let mut merged_fields: syn::punctuated::Punctuated<syn::Field, syn::Token![,]> =
        syn::punctuated::Punctuated::new();
    for f in &parent_named.named {
        merged_fields.push(f.clone());
    }
    for f in &child_named.named {
        merged_fields.push(f.clone());
    }
    child_named.named = merged_fields;

    // --- Merge methods: add parent methods not overridden by child ---
    // Collect child method names
    let child_method_names: std::collections::HashSet<String> = child_impl
        .items
        .iter()
        .filter_map(|item| {
            if let syn::ImplItem::Fn(m) = item {
                if matches!(m.vis, syn::Visibility::Public(_)) {
                    return Some(m.sig.ident.to_string());
                }
            }
            None
        })
        .collect();

    // Add parent public methods that the child doesn't override, skipping constructor
    for item in &parent_impl.items {
        if let syn::ImplItem::Fn(method) = item {
            if !matches!(method.vis, syn::Visibility::Public(_)) {
                continue;
            }
            let method_name = method.sig.ident.to_string();
            if method_name == "constructor" {
                continue;
            }
            if !child_method_names.contains(&method_name) {
                child_impl.items.push(syn::ImplItem::Fn(method.clone()));
            }
        }
    }

    Ok(())
}
