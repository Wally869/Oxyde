pub mod abi;
pub mod artifact;
pub mod bytecode;
pub mod compiler;
pub mod layout;
pub mod opcodes;
pub mod types;

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
