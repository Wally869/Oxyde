use alloy_primitives::{Address, U256};
use revm::{
    ExecuteCommitEvm, MainBuilder, MainContext,
    context::{Context, TxEnv},
    database::InMemoryDB,
    primitives::TxKind,
    context_interface::result::{ExecutionResult, Output},
};

use crate::errors::TestEnvError;

pub struct TestEnv {
    db: InMemoryDB,
}

impl TestEnv {
    pub fn new() -> Self {
        TestEnv {
            db: InMemoryDB::default(),
        }
    }

    /// Deploy a contract. `bytecode` should be creation_bytecode + ABI-encoded constructor args.
    /// Returns the deployed contract address.
    pub fn deploy(&mut self, bytecode: &[u8]) -> Result<Address, TestEnvError> {
        let mut evm = Context::mainnet()
            .with_db(&mut self.db)
            .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
            .build_mainnet();

        let tx = TxEnv::builder()
            .caller(Address::ZERO)
            .gas_limit(10_000_000)
            .kind(TxKind::Create)
            .data(revm::primitives::Bytes::copy_from_slice(bytecode))
            .value(U256::ZERO)
            .build()
            .map_err(|e| TestEnvError::DeployFailed(e.to_string()))?;

        let result = evm
            .transact_commit(tx)
            .map_err(|e| TestEnvError::DeployFailed(format!("{e:?}")))?;

        match result {
            ExecutionResult::Success { output, .. } => match output {
                Output::Create(_, Some(addr)) => Ok(addr),
                _ => Err(TestEnvError::NoAddress),
            },
            ExecutionResult::Revert { output, .. } => {
                let hex: String = output.iter().map(|b| format!("{b:02x}")).collect();
                Err(TestEnvError::Reverted(format!("0x{hex}")))
            }
            ExecutionResult::Halt { reason, .. } => {
                Err(TestEnvError::Halted(format!("{reason:?}")))
            }
        }
    }

    /// Call a contract with the given calldata (selector + ABI-encoded args).
    /// This is a state-mutating call.
    pub fn call(
        &mut self,
        caller: Address,
        contract: Address,
        calldata: &[u8],
    ) -> Result<Vec<u8>, TestEnvError> {
        let mut evm = Context::mainnet()
            .with_db(&mut self.db)
            .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
            .build_mainnet();

        let tx = TxEnv::builder()
            .caller(caller)
            .gas_limit(10_000_000)
            .kind(TxKind::Call(contract))
            .data(revm::primitives::Bytes::copy_from_slice(calldata))
            .value(U256::ZERO)
            .build()
            .map_err(|e| TestEnvError::DeployFailed(e.to_string()))?;

        let result = evm
            .transact_commit(tx)
            .map_err(|e| TestEnvError::Halted(format!("{e:?}")))?;

        match result {
            ExecutionResult::Success { output, .. } => Ok(output.into_data().to_vec()),
            ExecutionResult::Revert { output, .. } => {
                let hex: String = output.iter().map(|b| format!("{b:02x}")).collect();
                Err(TestEnvError::Reverted(format!("0x{hex}")))
            }
            ExecutionResult::Halt { reason, .. } => {
                Err(TestEnvError::Halted(format!("{reason:?}")))
            }
        }
    }

    /// Static call (read-only). Same as call but does not mutate state.
    /// For view functions.
    pub fn static_call(
        &mut self,
        contract: Address,
        calldata: &[u8],
    ) -> Result<Vec<u8>, TestEnvError> {
        self.call(Address::ZERO, contract, calldata)
    }

    /// Set the ETH balance of an address (useful for testing payable functions later).
    pub fn set_balance(&mut self, address: Address, balance: U256) {
        use revm::state::AccountInfo;

        let mut info = AccountInfo::default();
        info.balance = balance;
        self.db.insert_account_info(address, info);
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}
