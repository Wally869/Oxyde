use oxyde_sdk::{load_contract, Address, TestEnv, U256};

load_contract!(ERC20);
load_contract!(Vault);

fn alice() -> Address {
    Address::left_padding_from(&[0x01])
}

fn alice_u256() -> U256 {
    U256::from(0x01u64)
}

fn addr_to_u256(addr: Address) -> U256 {
    U256::from_be_bytes(addr.into_word().0)
}

fn deploy_token_and_vault(env: &mut TestEnv) -> (ERC20, Vault) {
    let token = ERC20::deploy(env, U256::from(0)).unwrap();
    let token_addr_u256 = addr_to_u256(token.address());
    let vault = Vault::deploy(env, token_addr_u256).unwrap();
    (token, vault)
}

#[test]
fn vault_stores_token_address() {
    let mut env = TestEnv::new();
    let (token, vault) = deploy_token_and_vault(&mut env);
    let stored = vault.token_address(&mut env).unwrap();
    assert_eq!(stored, addr_to_u256(token.address()));
}

#[test]
fn vault_deposit_transfers_tokens() {
    let mut env = TestEnv::new();
    let (token, vault) = deploy_token_and_vault(&mut env);

    // Mint tokens to alice
    token.mint(&mut env, alice(), U256::from(1000)).unwrap();
    assert_eq!(
        token.balance_of(&mut env, alice()).unwrap(),
        U256::from(1000)
    );

    // Alice approves vault to spend her tokens
    let token_as_alice = token.with_sender(alice());
    token_as_alice
        .approve(&mut env, vault.address(), U256::from(500))
        .unwrap();

    // Alice deposits 500 tokens into vault
    let vault_as_alice = vault.with_sender(alice());
    vault_as_alice.deposit(&mut env, U256::from(500)).unwrap();

    // Alice's token balance decreased
    assert_eq!(
        token_as_alice.balance_of(&mut env, alice()).unwrap(),
        U256::from(500)
    );
    // Vault holds the tokens
    assert_eq!(
        token_as_alice.balance_of(&mut env, vault_as_alice.address()).unwrap(),
        U256::from(500)
    );
    // Vault tracks alice's deposit
    assert_eq!(
        vault_as_alice.deposit_of(&mut env, alice_u256()).unwrap(),
        U256::from(500)
    );
}

#[test]
fn vault_withdraw_returns_tokens() {
    let mut env = TestEnv::new();
    let (token, vault) = deploy_token_and_vault(&mut env);

    // Setup: mint and deposit
    token.mint(&mut env, alice(), U256::from(1000)).unwrap();
    let token_as_alice = token.with_sender(alice());
    token_as_alice
        .approve(&mut env, vault.address(), U256::from(1000))
        .unwrap();
    let vault_as_alice = vault.with_sender(alice());
    vault_as_alice.deposit(&mut env, U256::from(800)).unwrap();

    // Withdraw 300
    vault_as_alice.withdraw(&mut env, U256::from(300)).unwrap();

    // Alice got 300 tokens back
    assert_eq!(
        token_as_alice.balance_of(&mut env, alice()).unwrap(),
        U256::from(500) // 1000 - 800 + 300
    );
    // Vault has 500 left
    assert_eq!(
        token_as_alice.balance_of(&mut env, vault_as_alice.address()).unwrap(),
        U256::from(500)
    );
    // Deposit tracking updated
    assert_eq!(
        vault_as_alice.deposit_of(&mut env, alice_u256()).unwrap(),
        U256::from(500) // 800 - 300
    );
}

#[test]
fn vault_withdraw_reverts_on_insufficient_deposit() {
    let mut env = TestEnv::new();
    let (_token, vault) = deploy_token_and_vault(&mut env);

    let vault_as_alice = vault.with_sender(alice());
    // Alice has no deposit, should revert
    let result = vault_as_alice.withdraw(&mut env, U256::from(100));
    assert!(result.is_err(), "should revert on insufficient deposit");
}
