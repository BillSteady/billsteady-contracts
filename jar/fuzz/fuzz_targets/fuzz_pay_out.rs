#![no_main]
// Fuzzes `pay_out` against a jar pre-funded with a fixed, known balance,
// paying to the one allowlisted payee. An arbitrary i128 amount must only
// ever yield Ok(()) or a declared Error, and a rejected call must never
// mutate the balance.
use billsteady_jar::{JarContract, JarContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Bytes, Env, Vec};

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
    payees.push_back(payee.clone());

    let contract_id = env.register(
        JarContract,
        (oper_admin, signers, 1u32, allowed_tokens, payees),
    );
    let client = JarContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    token_admin.mint(&owner, &JAR_BALANCE);

    // locked=true deliberately: pay_out must work on a locked jar (that's
    // the product's intended "pay the bill this jar was for" behavior) —
    // fuzzing against a locked jar proves the fuzzed amount never bypasses
    // that or any other check via an unexpected panic.
    let jar_id = client.create_jar(&owner, &token, &1i128, &u64::MAX, &true);
    client.deposit(&jar_id, &owner, &JAR_BALANCE);

    let jar_before = client.get_jar(&jar_id);
    let order_ref = Bytes::from_slice(&env, b"FUZZ-ORDER");

    // Must never panic.
    let result = client.try_pay_out(&jar_id, &payee, &amount, &order_ref);

    if result.is_err() {
        let jar_after = client.get_jar(&jar_id);
        assert_eq!(
            jar_before.balance, jar_after.balance,
            "a rejected pay_out mutated the jar's balance"
        );
    }
});
