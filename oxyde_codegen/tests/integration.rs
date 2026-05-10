use oxyde_codegen::compile_contract_source;
use revm::{
    ExecuteCommitEvm, MainBuilder, MainContext,
    context::{Context, TxEnv},
    database::InMemoryDB,
    primitives::{TxKind, Address, U256, Bytes},
    context_interface::result::{ExecutionResult, Output},
};

// The Counter contract source: a struct with a storage slot and two methods.
const COUNTER_SOURCE: &str = r#"
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
"#;

/// Deploy `bytecode` using a CREATE transaction into `db`.
/// Returns the address of the deployed contract.
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
        ExecutionResult::Halt { reason, .. } => {
            panic!("deploy halted: {:?}", reason);
        }
    }
}

/// Call a deployed contract at `to` with `calldata`, returning the output bytes.
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
        ExecutionResult::Halt { reason, .. } => {
            panic!("call halted: {:?}", reason);
        }
    }
}

#[test]
fn compile_and_deploy_counter() {
    // 1. Compile
    let artifact = compile_contract_source(COUNTER_SOURCE).expect("compilation failed");

    assert_eq!(artifact.name, "Counter");
    assert!(
        artifact.functions.iter().any(|f| f.name == "get_value"),
        "get_value not found in ABI"
    );
    assert!(
        artifact.functions.iter().any(|f| f.name == "increment"),
        "increment not found in ABI"
    );

    // 2. Build deployment calldata: creation_bytecode + ABI-encoded constructor arg (42)
    let mut constructor_arg = [0u8; 32];
    constructor_arg[31] = 42u8; // U256(42) big-endian
    let mut deploy_data = artifact.creation_bytecode.clone();
    deploy_data.extend_from_slice(&constructor_arg);

    // Print bytecode for debugging if needed
    let hex: String = deploy_data.iter().map(|b| format!("{b:02x}")).collect();
    println!("creation bytecode (with constructor arg): 0x{hex}");

    // 3. Deploy
    let mut db = InMemoryDB::default();
    let contract_address = deploy(&mut db, &deploy_data);
    println!("deployed at: {contract_address:?}");

    // 4. get_value() → should return 42
    let get_value_selector = artifact
        .functions
        .iter()
        .find(|f| f.name == "get_value")
        .unwrap()
        .selector;
    let return_data = call(&mut db, contract_address, &get_value_selector);
    println!("get_value() returned {} bytes", return_data.len());

    assert_eq!(return_data.len(), 32, "get_value() should return 32 bytes");
    let value = U256::from_be_slice(&return_data);
    assert_eq!(value, U256::from(42u64), "initial value should be 42, got {value}");

    // 5. increment()
    let increment_selector = artifact
        .functions
        .iter()
        .find(|f| f.name == "increment")
        .unwrap()
        .selector;
    let _empty = call(&mut db, contract_address, &increment_selector);

    // 6. get_value() → should return 43
    let return_data2 = call(&mut db, contract_address, &get_value_selector);
    assert_eq!(return_data2.len(), 32, "get_value() after increment should return 32 bytes");
    let value2 = U256::from_be_slice(&return_data2);
    assert_eq!(value2, U256::from(43u64), "value after increment should be 43, got {value2}");
}
