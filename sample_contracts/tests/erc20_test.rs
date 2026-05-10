use oxyde_sdk::{load_contract, Address, TestEnv, U256};

load_contract!(ERC20);

fn alice() -> Address {
    Address::left_padding_from(&[0x01])
}

fn bob() -> Address {
    Address::left_padding_from(&[0x02])
}

fn charlie() -> Address {
    Address::left_padding_from(&[0x03])
}

// ---- Basic queries ----

#[test]
fn deploy_sets_total_supply() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(1000)).unwrap();
    assert_eq!(token.total_supply(&mut env).unwrap(), U256::from(1000));
}

#[test]
fn balance_of_returns_zero_for_unknown_account() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::ZERO);
}

// ---- Mint ----

#[test]
fn mint_increases_balance_and_supply() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(500)).unwrap();
    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::from(500));
    assert_eq!(token.total_supply(&mut env).unwrap(), U256::from(500));
}

#[test]
fn mint_multiple_accounts() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(300)).unwrap();
    token.mint(&mut env, bob(), U256::from(200)).unwrap();
    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::from(300));
    assert_eq!(token.balance_of(&mut env, bob()).unwrap(), U256::from(200));
    assert_eq!(token.total_supply(&mut env).unwrap(), U256::from(500));
}

#[test]
fn mint_accumulates() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(100)).unwrap();
    token.mint(&mut env, alice(), U256::from(200)).unwrap();
    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::from(300));
}

// ---- Transfer ----

#[test]
fn transfer_moves_tokens() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(1000)).unwrap();

    let token = token.with_sender(alice());
    token.transfer(&mut env, bob(), U256::from(400)).unwrap();

    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::from(600));
    assert_eq!(token.balance_of(&mut env, bob()).unwrap(), U256::from(400));
    assert_eq!(token.total_supply(&mut env).unwrap(), U256::from(1000));
}

#[test]
fn transfer_insufficient_balance_reverts() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(100)).unwrap();

    let token = token.with_sender(alice());
    let result = token.transfer(&mut env, bob(), U256::from(200));
    assert!(result.is_err(), "should revert on insufficient balance");
}

#[test]
fn transfer_zero_amount_succeeds() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    let token = token.with_sender(alice());
    token.transfer(&mut env, bob(), U256::from(0)).unwrap();
}

// ---- Approve / Allowance ----

#[test]
fn approve_sets_allowance() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(500)).unwrap();

    assert_eq!(token.allowance(&mut env, alice(), bob()).unwrap(), U256::from(500));
}

#[test]
fn allowance_defaults_to_zero() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    assert_eq!(token.allowance(&mut env, alice(), bob()).unwrap(), U256::ZERO);
}

#[test]
fn approve_overwrites_previous() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(500)).unwrap();
    token.approve(&mut env, bob(), U256::from(200)).unwrap();

    assert_eq!(token.allowance(&mut env, alice(), bob()).unwrap(), U256::from(200));
}

#[test]
fn approve_is_directional() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(500)).unwrap();

    // alice→bob is 500, bob→alice is 0
    assert_eq!(token.allowance(&mut env, alice(), bob()).unwrap(), U256::from(500));
    assert_eq!(token.allowance(&mut env, bob(), alice()).unwrap(), U256::ZERO);
}

// ---- TransferFrom ----

#[test]
fn transfer_from_with_approval() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    // Mint to alice
    token.mint(&mut env, alice(), U256::from(1000)).unwrap();

    // Alice approves bob to spend 500
    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(500)).unwrap();

    // Bob transfers 300 from alice to charlie
    let token = token.with_sender(bob());
    token.transfer_from(&mut env, alice(), charlie(), U256::from(300)).unwrap();

    assert_eq!(token.balance_of(&mut env, alice()).unwrap(), U256::from(700));
    assert_eq!(token.balance_of(&mut env, charlie()).unwrap(), U256::from(300));
    assert_eq!(token.allowance(&mut env, alice(), bob()).unwrap(), U256::from(200));
}

#[test]
fn transfer_from_insufficient_allowance_reverts() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    token.mint(&mut env, alice(), U256::from(1000)).unwrap();

    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(100)).unwrap();

    let token = token.with_sender(bob());
    let result = token.transfer_from(&mut env, alice(), charlie(), U256::from(200));
    assert!(result.is_err(), "should revert on insufficient allowance");
}

#[test]
fn transfer_from_insufficient_balance_reverts() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();

    token.mint(&mut env, alice(), U256::from(50)).unwrap();

    let token = token.with_sender(alice());
    token.approve(&mut env, bob(), U256::from(1000)).unwrap();

    let token = token.with_sender(bob());
    let result = token.transfer_from(&mut env, alice(), charlie(), U256::from(100));
    assert!(result.is_err(), "should revert on insufficient balance");
}

#[test]
fn transfer_from_without_approval_reverts() {
    let mut env = TestEnv::new();
    let token = ERC20::deploy(&mut env, U256::from(0)).unwrap();
    token.mint(&mut env, alice(), U256::from(1000)).unwrap();

    let token = token.with_sender(bob());
    let result = token.transfer_from(&mut env, alice(), charlie(), U256::from(100));
    assert!(result.is_err(), "should revert without approval");
}

// ---- ABI ----

#[test]
fn erc20_abi_json_is_valid() {
    let parsed: serde_json::Value =
        serde_json::from_str(ABI_JSON).unwrap();
    let arr = parsed.as_array().unwrap();
    // constructor + total_supply + balance_of + allowance + mint + approve + transfer + transfer_from = 8
    assert_eq!(arr.len(), 8);
}
