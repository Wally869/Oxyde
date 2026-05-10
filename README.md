# Oxyde

A Rust-to-EVM bytecode compiler. Write smart contracts in Rust, compile them to real EVM bytecode, and test against REVM.

## How it works

```
Rust contract source (.rs)
  → build.rs invokes oxyde_codegen (syn parses AST)
  → emits EVM bytecode + Solidity-compatible ABI JSON
  → writes artifacts to OUT_DIR

Tests:
  → load_contract!(ContractName) includes generated artifacts
  → deploy bytecode into TestEnv (REVM)
  → call via typed wrappers with ABI encoding/decoding
```

## Example: ERC20

```rust
use oxyde_sdk::{msg_sender, Address, StorageDoubleMap, StorageItem, StorageMap, U256};

pub struct ERC20Base {
    pub total_supply: StorageItem<U256>,
    pub balances: StorageMap<Address, U256>,
    pub allowances: StorageDoubleMap<Address, Address, U256>,
}

impl ERC20Base {
    pub fn transfer(&mut self, to: Address, amount: U256) {
        let sender = msg_sender();
        let sender_bal = self.balances.get(&sender);
        if sender_bal < amount {
            panic!("insufficient balance");
        }
        self.balances.set_value(&sender, sender_bal - amount);
        self.balances.set_value(&to, self.balances.get(&to) + amount);
    }
}
```

## Example: Cross-contract calls

```rust
use oxyde_sdk::{self_address, msg_sender, StorageItem, StorageMap, U256};

pub struct Vault {
    pub token: StorageItem<U256>,
    pub deposits: StorageMap<U256, U256>,
}

impl Vault {
    pub fn deposit(&mut self, amount: U256) {
        let sender = msg_sender();
        let vault_addr = self_address();
        let token_addr = self.token.get_value();

        // Calls ERC20Base::transfer_from on the token contract
        ERC20Base::transfer_from(token_addr, sender, vault_addr, amount);

        self.deposits.set_value(&sender, self.deposits.get(&sender) + amount);
    }
}
```

The compiler resolves `ERC20Base::transfer_from(...)` by looking up the sibling contract's ABI at build time and generating the EVM CALL opcode sequence with the correct selector.

## Example: Testing

```rust
use oxyde_sdk::{load_contract, Address, TestEnv, U256};

load_contract!(ERC20);

#[test]
fn transfer_works() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(1000)).unwrap();

    let alice = Address::left_padding_from(&[0x01]);
    token.mint(&mut env, alice, U256::from(500)).unwrap();
    assert_eq!(token.balance_of(&mut env, alice).unwrap(), U256::from(500));
}
```

## Workspace

| Crate | Purpose |
|-------|---------|
| `oxyde_codegen` | Rust AST → EVM bytecode + ABI. Core compiler. |
| `oxyde_sdk` | Marker types (StorageItem, StorageMap), REVM test harness, intrinsics |
| `oxyde_macros` | Proc macros: `load_contract!`, `#[contract]`, `#[entrypoints]`, `#[extends]` |
| `oxyde_parser` | Utility types for AST extraction (Arg, Mutability) |
| `sample_contracts` | Counter, ERC20, Vault examples with integration tests |

## Features

- **Storage types**: `StorageItem<T>`, `StorageMap<K, V>`, `StorageDoubleMap<K1, K2, V>`
- **Contract inheritance**: `#[extends(ParentContract)]` merges parent fields and methods
- **Cross-contract calls**: `ContractName::method(target, args...)` with automatic selector resolution
- **Typed test wrappers**: Generated structs with deploy/call methods, ABI encode/decode
- **Intrinsics**: `msg_sender()`, `self_address()`, `msg_value()`
- **Events**: `emit_log1_raw`, `emit_log2_raw`, `emit_log3_raw` → LOG opcodes
- **Control flow**: if/else, let bindings, panic → REVERT

## Supported Rust patterns

| Rust | EVM |
|------|-----|
| `self.field.get_value()` | SLOAD |
| `self.field.set(expr)` | SSTORE |
| `self.map.get(&key)` | keccak256 slot + SLOAD |
| `self.map.set_value(&key, val)` | keccak256 slot + SSTORE |
| `msg_sender()` | CALLER |
| `self_address()` | ADDRESS |
| `msg_value()` | CALLVALUE |
| `a + b`, `a - b`, `a * b`, `a / b` | ADD, SUB, MUL, DIV |
| `a < b`, `a > b`, `a == b` | LT, GT, EQ |
| `if cond { ... }` | ISZERO + JUMPI |
| `panic!(...)` | REVERT |
| `Contract::method(target, ...)` | CALL with ABI-encoded calldata |
| `emit_logN_raw(topics..., data)` | LOGN |
