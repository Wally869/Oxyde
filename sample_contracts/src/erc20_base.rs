use oxyde_sdk::{msg_sender, Address, StorageDoubleMap, StorageItem, StorageMap, U256};

pub struct ERC20Base {
    pub total_supply: StorageItem<U256>,
    pub balances: StorageMap<Address, U256>,
    pub allowances: StorageDoubleMap<Address, Address, U256>,
}

impl ERC20Base {
    pub fn constructor(initial_supply: U256) -> Self {
        ERC20Base {
            total_supply: StorageItem::new_with_value("total_supply", initial_supply),
            balances: StorageMap::new("balances"),
            allowances: StorageDoubleMap::new("allowances"),
        }
    }

    pub fn total_supply(&self) -> U256 {
        self.total_supply.get_value()
    }

    pub fn balance_of(&self, account: Address) -> U256 {
        self.balances.get(&account)
    }

    pub fn allowance(&self, owner: Address, spender: Address) -> U256 {
        self.allowances.get(&owner, &spender)
    }

    pub fn mint(&mut self, to: Address, amount: U256) {
        self.balances.set_value(&to, self.balances.get(&to) + amount);
        self.total_supply.set(self.total_supply.get_value() + amount)
    }

    pub fn approve(&mut self, spender: Address, amount: U256) {
        let owner = msg_sender();
        self.allowances.set_value(&owner, &spender, amount)
    }

    pub fn transfer(&mut self, to: Address, amount: U256) {
        let sender = msg_sender();
        let sender_bal = self.balances.get(&sender);
        if sender_bal < amount {
            panic!("insufficient balance");
        }
        self.balances.set_value(&sender, sender_bal - amount);
        self.balances.set_value(&to, self.balances.get(&to) + amount)
    }

    pub fn transfer_from(&mut self, from: Address, to: Address, amount: U256) {
        let spender = msg_sender();
        let current_allowance = self.allowances.get(&from, &spender);
        if current_allowance < amount {
            panic!("insufficient allowance");
        }
        let sender_bal = self.balances.get(&from);
        if sender_bal < amount {
            panic!("insufficient balance");
        }
        self.allowances.set_value(&from, &spender, current_allowance - amount);
        self.balances.set_value(&from, sender_bal - amount);
        self.balances.set_value(&to, self.balances.get(&to) + amount)
    }
}
