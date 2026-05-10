#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageFieldKind {
    Item,
    Map,
    DoubleMap,
}

#[derive(Debug, Clone)]
pub struct StorageField {
    pub name: String,
    pub slot: u32,
    pub kind: StorageFieldKind,
    pub value_type: String,
    pub key_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StorageLayout {
    pub fields: Vec<StorageField>,
}

impl StorageLayout {
    pub fn from_struct(item: &syn::ItemStruct) -> Result<Self, String> {
        let named = match &item.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(format!(
                    "struct '{}' must have named fields",
                    item.ident
                ));
            }
        };

        let mut fields = Vec::new();
        let mut slot: u32 = 0;

        for field in named {
            let field_name = field
                .ident
                .as_ref()
                .ok_or_else(|| "unnamed field".to_string())?
                .to_string();

            let outer = extract_type_name(&field.ty).ok_or_else(|| {
                format!(
                    "field '{}': unsupported type — expected StorageItem or StorageMap",
                    field_name
                )
            })?;

            let args = extract_generic_args(&field.ty);

            let storage_field = match outer.as_str() {
                "StorageItem" => {
                    if args.len() != 1 {
                        return Err(format!(
                            "field '{}': StorageItem expects 1 generic argument, got {}",
                            field_name,
                            args.len()
                        ));
                    }
                    StorageField {
                        name: field_name,
                        slot,
                        kind: StorageFieldKind::Item,
                        value_type: args[0].clone(),
                        key_types: vec![],
                    }
                }
                "StorageMap" => {
                    if args.len() != 2 {
                        return Err(format!(
                            "field '{}': StorageMap expects 2 generic arguments, got {}",
                            field_name,
                            args.len()
                        ));
                    }
                    StorageField {
                        name: field_name,
                        slot,
                        kind: StorageFieldKind::Map,
                        key_types: vec![args[0].clone()],
                        value_type: args[1].clone(),
                    }
                }
                "StorageDoubleMap" => {
                    if args.len() != 3 {
                        return Err(format!(
                            "field '{}': StorageDoubleMap expects 3 generic arguments, got {}",
                            field_name,
                            args.len()
                        ));
                    }
                    StorageField {
                        name: field_name,
                        slot,
                        kind: StorageFieldKind::DoubleMap,
                        key_types: vec![args[0].clone(), args[1].clone()],
                        value_type: args[2].clone(),
                    }
                }
                other => {
                    return Err(format!(
                        "field '{}': unsupported storage type '{}' — expected StorageItem, StorageMap, or StorageDoubleMap",
                        field_name, other
                    ));
                }
            };

            fields.push(storage_field);
            slot += 1;
        }

        Ok(StorageLayout { fields })
    }

    pub fn get_field(&self, name: &str) -> Option<&StorageField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

pub fn extract_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        Some(segment.ident.to_string())
    } else {
        None
    }
}

pub fn extract_generic_args(ty: &syn::Type) -> Vec<String> {
    let syn::Type::Path(type_path) = ty else {
        return vec![];
    };
    let segment = match type_path.path.segments.last() {
        Some(s) => s,
        None => return vec![],
    };
    let syn::PathArguments::AngleBracketed(angle) = &segment.arguments else {
        return vec![];
    };

    angle
        .args
        .iter()
        .filter_map(|arg| {
            if let syn::GenericArgument::Type(inner_ty) = arg {
                Some(type_to_string(inner_ty))
            } else {
                None
            }
        })
        .collect()
}

fn type_to_string(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => {
            let segment = match type_path.path.segments.last() {
                Some(s) => s,
                None => return String::new(),
            };
            segment.ident.to_string()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_storage_item() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Counter {
                pub value: StorageItem<U256>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        assert_eq!(layout.fields.len(), 1);
        let f = &layout.fields[0];
        assert_eq!(f.name, "value");
        assert_eq!(f.slot, 0);
        assert_eq!(f.kind, StorageFieldKind::Item);
        assert_eq!(f.value_type, "U256");
        assert_eq!(f.key_types, Vec::<String>::new());
    }

    #[test]
    fn multiple_fields_sequential_slots() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Erc20 {
                pub name: StorageItem<String>,
                pub symbol: StorageItem<String>,
                pub decimals: StorageItem<u8>,
                pub admin: StorageItem<Address>,
                pub total_supply: StorageItem<U256>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        assert_eq!(layout.fields.len(), 5);
        for (i, field) in layout.fields.iter().enumerate() {
            assert_eq!(field.slot, i as u32);
            assert_eq!(field.kind, StorageFieldKind::Item);
        }
        assert_eq!(layout.fields[0].name, "name");
        assert_eq!(layout.fields[2].value_type, "u8");
        assert_eq!(layout.fields[3].value_type, "Address");
    }

    #[test]
    fn storage_map_kind_and_types() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Balances {
                pub balances: StorageMap<Address, U256>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        assert_eq!(layout.fields.len(), 1);
        let f = &layout.fields[0];
        assert_eq!(f.name, "balances");
        assert_eq!(f.slot, 0);
        assert_eq!(f.kind, StorageFieldKind::Map);
        assert_eq!(f.key_types, vec!["Address".to_string()]);
        assert_eq!(f.value_type, "U256");
    }

    #[test]
    fn mixed_item_and_map_slots() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Erc20 {
                pub name: StorageItem<String>,
                pub symbol: StorageItem<String>,
                pub decimals: StorageItem<u8>,
                pub admin: StorageItem<Address>,
                pub balances: StorageMap<Address, U256>,
                pub total_supply: StorageItem<U256>,
                pub allowances: StorageMap<String, U256>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        assert_eq!(layout.fields.len(), 7);

        assert_eq!(layout.fields[0].slot, 0);
        assert_eq!(layout.fields[0].kind, StorageFieldKind::Item);

        assert_eq!(layout.fields[4].name, "balances");
        assert_eq!(layout.fields[4].slot, 4);
        assert_eq!(layout.fields[4].kind, StorageFieldKind::Map);
        assert_eq!(layout.fields[4].key_types, vec!["Address".to_string()]);
        assert_eq!(layout.fields[4].value_type, "U256");

        assert_eq!(layout.fields[5].name, "total_supply");
        assert_eq!(layout.fields[5].slot, 5);
        assert_eq!(layout.fields[5].kind, StorageFieldKind::Item);

        assert_eq!(layout.fields[6].name, "allowances");
        assert_eq!(layout.fields[6].slot, 6);
        assert_eq!(layout.fields[6].kind, StorageFieldKind::Map);
        assert_eq!(layout.fields[6].key_types, vec!["String".to_string()]);
    }

    #[test]
    fn error_on_unsupported_type() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Bad {
                pub value: u64,
            }
        };
        let result = StorageLayout::from_struct(&item);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("unsupported storage type") || msg.contains("unsupported type"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn get_field_lookup() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Counter {
                pub value: StorageItem<U256>,
                pub owner: StorageItem<Address>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        let f = layout.get_field("owner").unwrap();
        assert_eq!(f.slot, 1);
        assert_eq!(f.value_type, "Address");
        assert!(layout.get_field("missing").is_none());
    }

    #[test]
    fn storage_double_map_kind_and_types() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Allowances {
                pub allowances: StorageDoubleMap<Address, Address, U256>,
            }
        };
        let layout = StorageLayout::from_struct(&item).unwrap();
        assert_eq!(layout.fields.len(), 1);
        let f = &layout.fields[0];
        assert_eq!(f.name, "allowances");
        assert_eq!(f.slot, 0);
        assert_eq!(f.kind, StorageFieldKind::DoubleMap);
        assert_eq!(f.key_types, vec!["Address".to_string(), "Address".to_string()]);
        assert_eq!(f.value_type, "U256");
    }
}
