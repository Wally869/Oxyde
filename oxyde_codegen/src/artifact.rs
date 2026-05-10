use crate::abi::{
    generate_abi_json, sol_type_decode_expr, sol_type_encode_expr, sol_type_to_rust_wrapper_type,
    FunctionAbi,
};
use crate::layout::StorageLayout;
use oxyde_parser::typing::Mutability;

pub struct ContractArtifact {
    pub name: String,
    pub creation_bytecode: Vec<u8>,
    pub runtime_bytecode: Vec<u8>,
    pub abi_json: String,
    pub functions: Vec<FunctionAbi>,
    pub constructor: Option<FunctionAbi>,
    pub layout: StorageLayout,
}

impl ContractArtifact {
    pub fn from_compiled(compiled: crate::compiler::CompiledContract) -> Self {
        let abi_json = generate_abi_json(&compiled.functions, compiled.constructor.as_ref());
        ContractArtifact {
            name: compiled.name,
            creation_bytecode: compiled.creation_bytecode,
            runtime_bytecode: compiled.runtime_bytecode,
            abi_json,
            functions: compiled.functions,
            constructor: compiled.constructor,
            layout: compiled.layout,
        }
    }

    /// Generate the typed wrapper struct + impl as a Rust source string.
    fn generate_wrapper_code(&self) -> String {
        let name = &self.name;
        let mut code = String::new();

        // Struct definition
        code.push_str(&format!(
            "pub struct {name} {{\n    \
               address: oxyde_sdk::Address,\n    \
               sender: oxyde_sdk::Address,\n\
             }}\n\n"
        ));

        code.push_str(&format!("impl {name} {{\n"));

        // deploy()
        {
            let ctor_params: Vec<String> = self
                .constructor
                .as_ref()
                .map(|c| {
                    c.inputs
                        .iter()
                        .map(|p| {
                            let rust_ty = sol_type_to_rust_wrapper_type(&p.param_type);
                            format!("{}: {rust_ty}", p.name)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let param_list = if ctor_params.is_empty() {
                String::new()
            } else {
                format!(", {}", ctor_params.join(", "))
            };

            let mut body = String::from(
                "        let mut creation = CREATION_BYTECODE.to_vec();\n",
            );

            if let Some(ctor) = &self.constructor {
                for param in &ctor.inputs {
                    let encode = sol_type_encode_expr(&param.param_type, &param.name);
                    body.push_str(&format!(
                        "        creation.extend_from_slice(&{encode});\n"
                    ));
                }
            }

            body.push_str(
                "        let address = env.deploy(&creation)?;\n        \
                 Ok(Self { address, sender: oxyde_sdk::Address::ZERO })\n",
            );

            code.push_str(&format!(
                "    pub fn deploy(env: &mut oxyde_sdk::TestEnv{param_list}) \
                 -> Result<Self, oxyde_sdk::TestEnvError> {{\n{body}    }}\n\n"
            ));
        }

        // address()
        code.push_str(
            "    pub fn address(&self) -> oxyde_sdk::Address { self.address }\n\n",
        );

        // with_sender()
        code.push_str(
            "    pub fn with_sender(mut self, sender: oxyde_sdk::Address) -> Self {\n        \
             self.sender = sender;\n        self\n    }\n\n",
        );

        // One method per ABI function
        for func in &self.functions {
            let fn_name = &func.name;
            let selector_const = format!("SELECTOR_{}", fn_name.to_uppercase());
            let is_view = matches!(func.mutability, Mutability::View | Mutability::Pure);

            // Build parameter list (excluding &self / env)
            let extra_params: Vec<String> = func
                .inputs
                .iter()
                .map(|p| {
                    let rust_ty = sol_type_to_rust_wrapper_type(&p.param_type);
                    format!("{}: {rust_ty}", p.name)
                })
                .collect();

            let param_list = if extra_params.is_empty() {
                String::new()
            } else {
                format!(", {}", extra_params.join(", "))
            };

            // Return type
            let return_type = if func.outputs.is_empty() {
                "()".to_string()
            } else {
                sol_type_to_rust_wrapper_type(&func.outputs[0].param_type).to_string()
            };

            let fn_sig = format!(
                "    pub fn {fn_name}(&self, env: &mut oxyde_sdk::TestEnv{param_list}) \
                 -> Result<{return_type}, oxyde_sdk::TestEnvError>"
            );

            let has_return = !func.outputs.is_empty();
            let capture = if has_return { "let result = " } else { "" };

            let mut body = String::new();
            if func.inputs.is_empty() {
                if is_view {
                    body.push_str(&format!(
                        "        {capture}env.static_call(self.address, &{selector_const})?;\n"
                    ));
                } else {
                    body.push_str(&format!(
                        "        {capture}env.call(self.sender, self.address, &{selector_const})?;\n"
                    ));
                }
            } else {
                body.push_str(&format!(
                    "        let mut calldata = {selector_const}.to_vec();\n"
                ));
                for param in &func.inputs {
                    let encode = sol_type_encode_expr(&param.param_type, &param.name);
                    body.push_str(&format!(
                        "        calldata.extend_from_slice(&{encode});\n"
                    ));
                }
                if is_view {
                    body.push_str(&format!(
                        "        {capture}env.static_call(self.address, &calldata)?;\n",
                    ));
                } else {
                    body.push_str(&format!(
                        "        {capture}env.call(self.sender, self.address, &calldata)?;\n",
                    ));
                }
            }

            // Return value
            if func.outputs.is_empty() {
                body.push_str("        Ok(())\n");
            } else {
                let decode =
                    sol_type_decode_expr(&func.outputs[0].param_type, "result", "0");
                body.push_str(&format!("        Ok({decode})\n"));
            }

            code.push_str(&format!("{fn_sig} {{\n{body}    }}\n\n"));
        }

        code.push_str("}\n");
        code
    }

    /// Write artifacts to a directory: `<name>.bin`, `<name>.abi`, and `<name>.rs`.
    pub fn write_to(&self, dir: &str) -> std::io::Result<()> {
        use std::fs;
        use std::path::Path;

        let dir = Path::new(dir);

        fs::write(dir.join(format!("{}.bin", self.name)), &self.creation_bytecode)?;
        fs::write(dir.join(format!("{}.abi", self.name)), &self.abi_json)?;

        let mut selectors = String::new();
        for func in &self.functions {
            let const_name = func.name.to_uppercase();
            selectors.push_str(&format!(
                "pub const SELECTOR_{}: [u8; 4] = [0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x}];\n",
                const_name,
                func.selector[0],
                func.selector[1],
                func.selector[2],
                func.selector[3],
            ));
        }

        let wrapper = self.generate_wrapper_code();

        let rs_content = format!(
            "// Auto-generated by oxyde_codegen\n\
             pub const CREATION_BYTECODE: &[u8] = include_bytes!(\"{name}.bin\");\n\
             pub const ABI_JSON: &str = include_str!(\"{name}.abi\");\n\
             {selectors}\n\
             {wrapper}",
            name = self.name,
            selectors = selectors,
            wrapper = wrapper,
        );

        fs::write(dir.join(format!("{}.rs", self.name)), rs_content)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler;

    #[test]
    fn from_compiled_produces_valid_json() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Counter {
                pub value: StorageItem<U256>,
            }
        };
        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Counter {
                pub fn get_value(&self) -> U256 {
                    self.value.get_value()
                }
            }
        };
        let compiled = compiler::compile(&contract_struct, &contract_impl).unwrap();
        let artifact = ContractArtifact::from_compiled(compiled);

        assert_eq!(artifact.name, "Counter");
        assert!(!artifact.creation_bytecode.is_empty());
        assert!(!artifact.runtime_bytecode.is_empty());

        let parsed: serde_json::Value = serde_json::from_str(&artifact.abi_json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }
}
