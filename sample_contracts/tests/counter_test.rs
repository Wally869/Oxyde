mod counter_artifacts {
    include!(concat!(env!("OUT_DIR"), "/Counter.rs"));
}

use alloy_primitives::U256;
use revm::{
    ExecuteCommitEvm, MainBuilder, MainContext,
    context::{Context, TxEnv},
    database::InMemoryDB,
    primitives::{TxKind, Address, Bytes},
    context_interface::result::{ExecutionResult, Output},
};

fn deploy(db: &mut InMemoryDB, bytecode: &[u8]) -> Address {
    let mut evm = Context::mainnet()
        .with_db(db as &mut InMemoryDB)
        .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
        .build_mainnet();

    let tx = TxEnv::builder()
        .caller(Address::ZERO)
        .gas_limit(10_000_000)
        .kind(TxKind::Create)
        .data(Bytes::copy_from_slice(bytecode))
        .value(U256::ZERO)
        .build()
        .expect("build deploy tx");

    let result = evm.transact_commit(tx).expect("deploy transact_commit");

    match result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, Some(addr)) => addr,
            other => panic!("deploy did not produce an address: {:?}", other),
        },
        ExecutionResult::Revert { output, .. } => {
            let hex: String = output.iter().map(|b| format!("{b:02x}")).collect();
            panic!("deploy reverted: 0x{hex}");
        }
        ExecutionResult::Halt { reason, .. } => panic!("deploy halted: {:?}", reason),
    }
}

fn call(db: &mut InMemoryDB, to: Address, calldata: &[u8]) -> Vec<u8> {
    let mut evm = Context::mainnet()
        .with_db(db as &mut InMemoryDB)
        .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
        .build_mainnet();

    let tx = TxEnv::builder()
        .caller(Address::ZERO)
        .gas_limit(10_000_000)
        .kind(TxKind::Call(to))
        .data(Bytes::copy_from_slice(calldata))
        .value(U256::ZERO)
        .build()
        .expect("build call tx");

    let result = evm.transact_commit(tx).expect("call transact_commit");

    match result {
        ExecutionResult::Success { output, .. } => output.into_data().to_vec(),
        ExecutionResult::Revert { output, .. } => {
            let hex: String = output.iter().map(|b| format!("{b:02x}")).collect();
            panic!("call reverted: 0x{hex}");
        }
        ExecutionResult::Halt { reason, .. } => panic!("call halted: {:?}", reason),
    }
}

#[test]
fn counter_via_build_script_artifacts() {
    let mut db = InMemoryDB::default();

    // Deploy with constructor arg = 100
    let mut deploy_data = counter_artifacts::CREATION_BYTECODE.to_vec();
    let mut arg = [0u8; 32];
    arg[31] = 100;
    deploy_data.extend_from_slice(&arg);

    let addr = deploy(&mut db, &deploy_data);

    // get_value() should return 100
    let result = call(&mut db, addr, &counter_artifacts::SELECTOR_GET_VALUE);
    assert_eq!(result.len(), 32);
    let val = U256::from_be_slice(&result);
    assert_eq!(val, U256::from(100));

    // increment()
    call(&mut db, addr, &counter_artifacts::SELECTOR_INCREMENT);

    // get_value() should return 101
    let result = call(&mut db, addr, &counter_artifacts::SELECTOR_GET_VALUE);
    let val = U256::from_be_slice(&result);
    assert_eq!(val, U256::from(101));
}

#[test]
fn counter_abi_json_is_valid() {
    let parsed: serde_json::Value = serde_json::from_str(counter_artifacts::ABI_JSON).unwrap();
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    // Should have constructor + get_value + increment = 3 entries
    assert_eq!(arr.len(), 3);
}
