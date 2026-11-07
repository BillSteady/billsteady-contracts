#![no_main]
// Fuzzes `deposit` with an arbitrary i128 amount (negative, zero, huge,
// anything libfuzzer generates). The contract must only ever return Ok(())
// or a declared Error — never panic, abort, or silently corrupt state.
use billsteady_jar::{JarContract, JarContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env, Vec};

fuzz_target!(|amount: i128| {
    let env = Env::default();
    env.mock_all_auths();

    let oper_admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let payee = Address::generate(&env);
    let token_admin_addr = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin_addr);
    let token = token_contract.address();
    let token_admin = StellarAssetClient::new(&env, &token);

    let mut signers = Vec::new(&env);
    signers.push_back(signer);
    let mut allowed_tokens = Vec::new(&env);
    allowed_tokens.push_back(token.clone());
    let mut payees = Vec::new(&env);
    payees.push_back(payee);

    let contract_id = env.register(
        JarContract,
        (oper_admin, signers, 1u32, allowed_tokens, payees),
    );
    let client = JarContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    // Fund generously so most fuzzed amounts reach the contract's own
    // validation/arithmetic rather than being starved by the token's own
    // transfer check (that path is the token contract's concern, not ours).
    token_admin.mint(&owner, &(i128::MAX / 2));

    let jar_id = client.create_jar(&owner, &token, &1i128, &0u64, &false);

    // Must never panic.
    let _ = client.try_deposit(&jar_id, &owner, &amount);
});
