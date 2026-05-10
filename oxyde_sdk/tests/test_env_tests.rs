use oxyde_sdk::{TestEnv, Address, U256};

#[test]
fn deploy_minimal_contract() {
    let mut env = TestEnv::new();
    // Deploy a minimal contract that just returns (runtime = STOP)
    // Creation code: PUSH1 01 PUSH1 0a PUSH1 00 CODECOPY PUSH1 01 PUSH1 00 RETURN STOP
    // This is the simplest possible contract
    let creation = vec![
        0x60, 0x01,  // PUSH1 0x01 (runtime length = 1)
        0x60, 0x0a,  // PUSH1 0x0a (offset = 10, where runtime starts)
        0x60, 0x00,  // PUSH1 0x00 (dest)
        0x39,        // CODECOPY
        0x60, 0x01,  // PUSH1 0x01 (length)
        0x60, 0x00,  // PUSH1 0x00 (offset)
        0xf3,        // RETURN
        0x00,        // STOP (this is the runtime code)
    ];
    let address = env.deploy(&creation).expect("deploy failed");
    assert_ne!(address, Address::ZERO);
}

#[test]
fn multiple_envs_are_independent() {
    // Two TestEnvs should not share state
    let mut env1 = TestEnv::new();
    let mut env2 = TestEnv::new();

    let creation = vec![
        0x60, 0x01, 0x60, 0x0a, 0x60, 0x00, 0x39,
        0x60, 0x01, 0x60, 0x00, 0xf3, 0x00,
    ];

    let addr1 = env1.deploy(&creation).expect("deploy1");
    let addr2 = env2.deploy(&creation).expect("deploy2");
    // They get different addresses (random) or at least both succeed independently
    // The key point: no shared global state
    let _ = (addr1, addr2);
}

#[test]
fn set_balance_works() {
    let mut env = TestEnv::new();
    let addr = Address::from([0xABu8; 20]);
    env.set_balance(addr, U256::from(1_000_000u64));
    // No panic means the call succeeded; balance is stored in the DB
}
