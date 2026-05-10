pub mod errors;
pub mod storage;
pub mod test_env;

pub use alloy_primitives::{Address, U256};
pub use storage::{StorageItem, StorageMap, StorageDoubleMap};
pub use test_env::TestEnv;
pub use errors::TestEnvError;
pub use oxyde_macros::load_contract;

/// Returns the address of the message sender.
/// This is a compiler intrinsic: the codegen replaces calls to this function
/// with the EVM CALLER opcode.
pub fn msg_sender() -> Address {
    Address::ZERO
}
