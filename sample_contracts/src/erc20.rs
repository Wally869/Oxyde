use oxyde_sdk::{Address, StorageDoubleMap, StorageItem, StorageMap, U256};

#[oxyde_macros::extends(ERC20Base)]
pub struct ERC20 {}

impl ERC20 {
    pub fn constructor(initial_supply: U256) -> Self {
        ERC20 {
            total_supply: StorageItem::new_with_value("total_supply", initial_supply),
            balances: StorageMap::new("balances"),
            allowances: StorageDoubleMap::new("allowances"),
        }
    }
}
