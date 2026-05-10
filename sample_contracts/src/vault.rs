use oxyde_sdk::{self_address, msg_sender, StorageItem, StorageMap, U256};

pub struct Vault {
    pub token: StorageItem<U256>,
    pub deposits: StorageMap<U256, U256>,
}

impl Vault {
    pub fn constructor(token_addr: U256) -> Self {
        Vault {
            token: StorageItem::new_with_value("token", token_addr),
            deposits: StorageMap::new("deposits"),
        }
    }

    pub fn token_address(&self) -> U256 {
        self.token.get_value()
    }

    pub fn deposit_of(&self, account: U256) -> U256 {
        self.deposits.get(&account)
    }

    pub fn deposit(&mut self, amount: U256) {
        let sender = msg_sender();
        let vault_addr = self_address();
        let token_addr = self.token.get_value();

        // ERC20Base.transfer_from(sender, vault, amount)
        ERC20Base::transfer_from(token_addr, sender, vault_addr, amount);

        self.deposits.set_value(&sender, self.deposits.get(&sender) + amount);
    }

    pub fn withdraw(&mut self, amount: U256) {
        let sender = msg_sender();
        let deposit = self.deposits.get(&sender);
        if deposit < amount {
            panic!("insufficient deposit");
        }

        self.deposits.set_value(&sender, deposit - amount);

        let token_addr = self.token.get_value();
        // ERC20Base.transfer(to, amount)
        ERC20Base::transfer(token_addr, sender, amount);
    }
}
