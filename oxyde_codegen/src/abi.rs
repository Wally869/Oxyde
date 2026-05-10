use serde::Serialize;
use sha3::{Digest, Keccak256};

use oxyde_parser::typing::{Arg, Mutability};

use crate::types::EvmType;

pub fn compute_selector(signature: &str) -> [u8; 4] {
    let hash = Keccak256::digest(signature.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

#[derive(Debug, Clone, Serialize)]
pub struct AbiEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub inputs: Vec<AbiParam>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<AbiParam>,
    #[serde(rename = "stateMutability")]
    pub state_mutability: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
}

pub fn syn_type_to_string(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

#[derive(Debug)]
pub struct FunctionAbi {
    pub name: String,
    pub selector: [u8; 4],
    pub inputs: Vec<AbiParam>,
    pub outputs: Vec<AbiParam>,
    pub mutability: Mutability,
}

impl FunctionAbi {
    pub fn new(
        name: &str,
        args: &[Arg],
        output_type: Option<&str>,
        mutability: Mutability,
    ) -> Result<Self, String> {
        let mut inputs = Vec::new();
        let mut sol_types = Vec::new();

        for arg in args {
            let rust_type_str = syn_type_to_string(&arg.r#type);
            let evm_type = EvmType::from_rust_type(&rust_type_str)?;
            sol_types.push(evm_type.solidity_name.clone());
            let param_name = arg
                .name
                .as_ref()
                .map(|ident| ident.to_string())
                .unwrap_or_default();
            inputs.push(AbiParam {
                name: param_name,
                param_type: evm_type.solidity_name,
            });
        }

        let signature = format!("{}({})", name, sol_types.join(","));
        let selector = compute_selector(&signature);

        let outputs = if let Some(out_type) = output_type {
            let evm_type = EvmType::from_rust_type(out_type)?;
            vec![AbiParam {
                name: String::new(),
                param_type: evm_type.solidity_name,
            }]
        } else {
            Vec::new()
        };

        Ok(FunctionAbi {
            name: name.to_string(),
            selector,
            inputs,
            outputs,
            mutability,
        })
    }

    pub fn to_abi_entry(&self) -> AbiEntry {
        let state_mutability = match self.mutability {
            Mutability::View => "view".to_string(),
            Mutability::Pure => "pure".to_string(),
            Mutability::Mutable => "nonpayable".to_string(),
        };
        AbiEntry {
            entry_type: "function".to_string(),
            name: Some(self.name.clone()),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            state_mutability,
        }
    }
}

pub fn generate_abi_json(functions: &[FunctionAbi], constructor: Option<&FunctionAbi>) -> String {
    let mut entries: Vec<AbiEntry> = Vec::new();
    if let Some(ctor) = constructor {
        let mut entry = ctor.to_abi_entry();
        entry.entry_type = "constructor".to_string();
        entry.name = None;
        entries.push(entry);
    }
    for func in functions {
        entries.push(func.to_abi_entry());
    }
    serde_json::to_string_pretty(&entries).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxyde_parser::typing::Mutability;

    #[test]
    fn selector_transfer() {
        let sel = compute_selector("transfer(address,uint256)");
        assert_eq!(sel, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn selector_balance_of() {
        let sel = compute_selector("balanceOf(address)");
        assert_eq!(sel, [0x70, 0xa0, 0x82, 0x31]);
    }

    #[test]
    fn function_abi_new_no_args_no_output() {
        let abi = FunctionAbi::new("myFunc", &[], None, Mutability::View).unwrap();
        assert_eq!(abi.name, "myFunc");
        assert!(abi.inputs.is_empty());
        assert!(abi.outputs.is_empty());
        let expected_sel = compute_selector("myFunc()");
        assert_eq!(abi.selector, expected_sel);
    }

    #[test]
    fn function_abi_to_abi_entry() {
        let abi = FunctionAbi::new("myFunc", &[], None, Mutability::Pure).unwrap();
        let entry = abi.to_abi_entry();
        assert_eq!(entry.entry_type, "function");
        assert_eq!(entry.name.as_deref(), Some("myFunc"));
        assert_eq!(entry.state_mutability, "pure");
    }

    #[test]
    fn generate_abi_json_produces_valid_json() {
        let abi = FunctionAbi::new("myFunc", &[], None, Mutability::View).unwrap();
        let json = generate_abi_json(&[abi], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }

    #[test]
    fn generate_abi_json_with_constructor() {
        let ctor = FunctionAbi::new("constructor", &[], None, Mutability::Mutable).unwrap();
        let func = FunctionAbi::new("myFunc", &[], None, Mutability::View).unwrap();
        let json = generate_abi_json(&[func], Some(&ctor));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "constructor");
        assert!(arr[0].get("name").is_none());
        assert_eq!(arr[1]["type"], "function");
    }
}
