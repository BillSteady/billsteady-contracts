#![no_main]
// Fuzzes `withdraw` against a jar pre-funded with a fixed, known balance.
// An arbitrary i128 withdraw amount (negative, zero, larger than balance,
// i128::MAX, ...) must only ever yield Ok(()) or a declared Error.
use billsteady_jar::{JarContract, JarContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env, Vec};

const JAR_BALANCE: i128 = 1_000_000_000_000;

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
    token_admin.mint(&owner, &JAR_BALANCE);

    // Unlocked (locked=false) so the withdraw path under test is reachable
    // regardless of the fuzzed amount — JarLocked would otherwise mask
    // every other rejection reason.
    let jar_id = client.create_jar(&owner, &token, &1i128, &0u64, &false);
    client.deposit(&jar_id, &owner, &JAR_BALANCE);

    let jar_before = client.get_jar(&jar_id);

    // Must never panic.
    let result = client.try_withdraw(&jar_id, &amount);

    // Specific invariant: a rejected withdraw must never have mutated the
    // balance (no partial effects before the error).
    if result.is_err() {
        let jar_after = client.get_jar(&jar_id);
        assert_eq!(
            jar_before.balance, jar_after.balance,
            "a rejected withdraw mutated the jar's balance"
        );
    }
});
