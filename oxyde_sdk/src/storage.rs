use std::marker::PhantomData;

/// Marker type for a single storage slot. Used by the codegen to identify
/// contract storage fields. Has no runtime behavior.
pub struct StorageItem<T> {
    _phantom: PhantomData<T>,
}

impl<T> StorageItem<T> {
    pub fn new(_key: &str) -> Self {
        StorageItem { _phantom: PhantomData }
    }

    pub fn new_with_value(_key: &str, _value: T) -> Self {
        StorageItem { _phantom: PhantomData }
    }

    pub fn get_value(&self) -> T where T: Default {
        T::default()
    }

    pub fn set(&mut self, _value: T) {}
}

/// Marker type for a storage mapping. Used by the codegen to identify
/// contract storage fields. Has no runtime behavior.
pub struct StorageMap<K, V> {
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> StorageMap<K, V> {
    pub fn new(_key: &str) -> Self {
        StorageMap { _phantom: PhantomData }
    }

    pub fn get(&self, _key: &K) -> V where V: Default {
        V::default()
    }

    pub fn set_value(&mut self, _key: &K, _value: V) {}
}
