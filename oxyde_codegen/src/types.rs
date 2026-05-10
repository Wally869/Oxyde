#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmType {
    pub solidity_name: String,
    pub size_bytes: usize,
    pub is_dynamic: bool,
}

impl EvmType {
    pub fn from_rust_type(rust_type: &str) -> Result<Self, String> {
        match rust_type {
            "U256" | "u256" => Ok(EvmType {
                solidity_name: "uint256".to_string(),
                size_bytes: 32,
                is_dynamic: false,
            }),
            "Address" => Ok(EvmType {
                solidity_name: "address".to_string(),
                size_bytes: 20,
                is_dynamic: false,
            }),
            "u8" => Ok(EvmType {
                solidity_name: "uint8".to_string(),
                size_bytes: 1,
                is_dynamic: false,
            }),
            "u16" => Ok(EvmType {
                solidity_name: "uint16".to_string(),
                size_bytes: 2,
                is_dynamic: false,
            }),
            "u32" => Ok(EvmType {
                solidity_name: "uint32".to_string(),
                size_bytes: 4,
                is_dynamic: false,
            }),
            "u64" => Ok(EvmType {
                solidity_name: "uint64".to_string(),
                size_bytes: 8,
                is_dynamic: false,
            }),
            "u128" => Ok(EvmType {
                solidity_name: "uint128".to_string(),
                size_bytes: 16,
                is_dynamic: false,
            }),
            "i8" => Ok(EvmType {
                solidity_name: "int8".to_string(),
                size_bytes: 1,
                is_dynamic: false,
            }),
            "i16" => Ok(EvmType {
                solidity_name: "int16".to_string(),
                size_bytes: 2,
                is_dynamic: false,
            }),
            "i32" => Ok(EvmType {
                solidity_name: "int32".to_string(),
                size_bytes: 4,
                is_dynamic: false,
            }),
            "i64" => Ok(EvmType {
                solidity_name: "int64".to_string(),
                size_bytes: 8,
                is_dynamic: false,
            }),
            "i128" => Ok(EvmType {
                solidity_name: "int128".to_string(),
                size_bytes: 16,
                is_dynamic: false,
            }),
            "bool" => Ok(EvmType {
                solidity_name: "bool".to_string(),
                size_bytes: 1,
                is_dynamic: false,
            }),
            "String" => Ok(EvmType {
                solidity_name: "string".to_string(),
                size_bytes: 32,
                is_dynamic: true,
            }),
            other => Err(format!("unsupported type: {}", other)),
        }
    }

    pub fn abi_slot_size(&self) -> usize {
        32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u256_maps_to_uint256() {
        let t = EvmType::from_rust_type("U256").unwrap();
        assert_eq!(t.solidity_name, "uint256");
        assert_eq!(t.size_bytes, 32);
        assert!(!t.is_dynamic);
    }

    #[test]
    fn address_maps_to_address() {
        let t = EvmType::from_rust_type("Address").unwrap();
        assert_eq!(t.solidity_name, "address");
        assert_eq!(t.size_bytes, 20);
        assert!(!t.is_dynamic);
    }

    #[test]
    fn u8_maps_to_uint8() {
        let t = EvmType::from_rust_type("u8").unwrap();
        assert_eq!(t.solidity_name, "uint8");
        assert_eq!(t.size_bytes, 1);
        assert!(!t.is_dynamic);
    }

    #[test]
    fn bool_maps_to_bool() {
        let t = EvmType::from_rust_type("bool").unwrap();
        assert_eq!(t.solidity_name, "bool");
        assert_eq!(t.size_bytes, 1);
        assert!(!t.is_dynamic);
    }

    #[test]
    fn string_is_dynamic() {
        let t = EvmType::from_rust_type("String").unwrap();
        assert_eq!(t.solidity_name, "string");
        assert!(t.is_dynamic);
    }

    #[test]
    fn unknown_type_returns_err() {
        let result = EvmType::from_rust_type("MyStruct");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "unsupported type: MyStruct");
    }
}
