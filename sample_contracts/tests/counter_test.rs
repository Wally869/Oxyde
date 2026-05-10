use oxyde_sdk::{load_contract, TestEnv, U256};

load_contract!(Counter);

#[test]
fn counter_deploy_and_read() {
    let mut env = TestEnv::new();
    let counter = Counter::deploy(&mut env, U256::from(100)).unwrap();
    assert_eq!(counter.get_value(&mut env).unwrap(), U256::from(100));
}

#[test]
fn counter_increment() {
    let mut env = TestEnv::new();
    let counter = Counter::deploy(&mut env, U256::from(42)).unwrap();
    counter.increment(&mut env).unwrap();
    assert_eq!(counter.get_value(&mut env).unwrap(), U256::from(43));
}

#[test]
fn counter_multiple_increments() {
    let mut env = TestEnv::new();
    let counter = Counter::deploy(&mut env, U256::from(0)).unwrap();
    for _ in 0..5 {
        counter.increment(&mut env).unwrap();
    }
    assert_eq!(counter.get_value(&mut env).unwrap(), U256::from(5));
}

#[test]
fn counter_abi_json_is_valid() {
    let parsed: serde_json::Value =
        serde_json::from_str(ABI_JSON).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 3); // constructor + get_value + increment
}
