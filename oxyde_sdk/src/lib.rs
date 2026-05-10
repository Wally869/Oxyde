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

/// Returns the address of this contract.
/// This is a compiler intrinsic: the codegen replaces calls to this function
/// with the EVM ADDRESS opcode.
pub fn self_address() -> Address {
    Address::ZERO
}

/// Returns the value (in wei) sent with the current call.
/// This is a compiler intrinsic: the codegen replaces calls to this function
/// with the EVM CALLVALUE opcode.
pub fn msg_value() -> U256 {
    U256::ZERO
}

/// Emit a LOG1 event: topic0 (32 bytes), data (32 bytes stored at mem[0..32]).
/// This is a compiler intrinsic: the codegen replaces calls with LOG1 opcode sequences.
pub fn emit_log1_raw(topic0: U256, data: U256) {
    let _ = (topic0, data);
}

/// Emit a LOG2 event: topic0 + topic1 as indexed, data as non-indexed.
/// This is a compiler intrinsic: the codegen replaces calls with LOG2 opcode sequences.
pub fn emit_log2_raw(topic0: U256, topic1: U256, data: U256) {
    let _ = (topic0, topic1, data);
}

/// Emit a LOG3 event: topic0 + topic1 + topic2 as indexed, data as non-indexed.
/// This is a compiler intrinsic: the codegen replaces calls with LOG3 opcode sequences.
pub fn emit_log3_raw(topic0: U256, topic1: U256, topic2: U256, data: U256) {
    let _ = (topic0, topic1, topic2, data);
}

/// Low-level cross-contract call helper. All methods are compiler intrinsics:
/// the codegen generates the EVM CALL opcode sequence.
pub struct ExternalCall;

impl ExternalCall {
    pub fn call_1(target: U256, selector: u32, arg0: U256) -> U256 {
        let _ = (target, selector, arg0);
        U256::ZERO
    }
    pub fn call_2(target: U256, selector: u32, arg0: U256, arg1: U256) -> U256 {
        let _ = (target, selector, arg0, arg1);
        U256::ZERO
    }
    pub fn call_3(target: U256, selector: u32, arg0: U256, arg1: U256, arg2: U256) -> U256 {
        let _ = (target, selector, arg0, arg1, arg2);
        U256::ZERO
    }
    pub fn call_void_1(target: U256, selector: u32, arg0: U256) {
        let _ = (target, selector, arg0);
    }
    pub fn call_void_2(target: U256, selector: u32, arg0: U256, arg1: U256) {
        let _ = (target, selector, arg0, arg1);
    }
    pub fn call_void_3(target: U256, selector: u32, arg0: U256, arg1: U256, arg2: U256) {
        let _ = (target, selector, arg0, arg1, arg2);
    }
}
