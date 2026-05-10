use oxyde_sdk::{StorageItem, U256};

pub struct Counter {
    pub value: StorageItem<U256>,
}

impl Counter {
    pub fn constructor(initial: U256) -> Self {
        Counter { value: StorageItem::new_with_value("value", initial) }
    }

    pub fn get_value(&self) -> U256 {
        self.value.get_value()
    }

    pub fn increment(&mut self) {
        self.value.set(self.value.get_value() + U256::from(1))
    }
}
