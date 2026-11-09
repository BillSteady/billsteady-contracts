#![cfg(test)]
extern crate std;

use crate::{BillPaid, BillPaymentRouter, BillPaymentRouterClient, Error};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Events, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env, Event, IntoVal, Symbol, Vec,
};

/// 3 configured admin signers, 2-of-3 threshold - same ratio as the
/// archived jar contract's upgrade gate, for consistency, though the
/// mechanism here is a single-transaction multisig, not jar's
/// multi-transaction propose/approve/timelock flow (see router/README.md
/// for why that's judged sufficient for a data-mapping change).
const THRESHOLD: u32 = 2;

struct Harness<'a> {
    env: Env,
    client: BillPaymentRouterClient<'a>,
    contract_id: Address,
    token: Address,
    token_admin: StellarAssetClient<'a>,
    signers: Vec<Address>, // length 3
}

fn setup() -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));

    let token_admin_addr = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin_addr);
    let token = token_contract.address();
    let token_admin = StellarAssetClient::new(&env, &token);

    let contract_id = env.register(BillPaymentRouter, (signers.clone(), THRESHOLD));
    let client = BillPaymentRouterClient::new(&env, &contract_id);

    Harness {
        env,
        client,
        contract_id,
        token,
        token_admin,
        signers,
    }
}

fn fund(h: &Harness, who: &Address, amount: i128) {
    h.token_admin.mint(who, &amount);
}

fn order_ref(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn subset(env: &Env, from: &Vec<Address>, indices: &[usize]) -> Vec<Address> {
    let mut out: Vec<Address> = Vec::new(env);
    for &i in indices {
        out.push_back(from.get(i as u32).unwrap());
    }
    out
}

/// Exactly `THRESHOLD` valid signers - the minimum quorum, for tests that
/// just need to register a biller and don't care about threshold mechanics
/// themselves.
fn quorum(h: &Harness) -> Vec<Address> {
    subset(&h.env, &h.signers, &[0, 1])
}

// ---------- constructor ----------

#[test]
fn constructor_rejects_threshold_of_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.register(BillPaymentRouter, (signers, 0u32));
    }));
    assert!(result.is_err(), "threshold of 0 must be rejected at construction");
}

#[test]
fn constructor_rejects_threshold_greater_than_signer_count() {
    let env = Env::default();
    env.mock_all_auths();
    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.register(BillPaymentRouter, (signers, 3u32));
    }));
    assert!(
        result.is_err(),
        "threshold greater than the number of configured signers must be rejected"
    );
}

#[test]
fn get_admin_config_returns_configured_signers_and_threshold() {
    let h = setup();
    let (signers, threshold) = h.client.get_admin_config();
    assert_eq!(signers, h.signers);
    assert_eq!(threshold, THRESHOLD);
}

// ---------- settlement address admin: threshold enforcement ----------

#[test]
fn set_settlement_address_succeeds_with_exactly_threshold_signers() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);

    h.client
        .set_settlement_address(&quorum(&h), &biller, &settlement);

    assert_eq!(h.client.get_settlement_address(&biller), settlement);
}

#[test]
fn set_settlement_address_succeeds_with_more_than_threshold_signers() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    let all_three = subset(&h.env, &h.signers, &[0, 1, 2]);

    h.client
        .set_settlement_address(&all_three, &biller, &settlement);

    assert_eq!(h.client.get_settlement_address(&biller), settlement);
}

#[test]
fn set_settlement_address_rejects_below_threshold_signers() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    let only_one = subset(&h.env, &h.signers, &[0]);

    let res = h
        .client
        .try_set_settlement_address(&only_one, &biller, &settlement);
    assert_eq!(res, Err(Ok(Error::ThresholdNotMet)));
    assert_eq!(
        h.client.try_get_settlement_address(&biller),
        Err(Ok(Error::UnknownBiller)),
        "a rejected call must not have written anything"
    );
}

#[test]
fn set_settlement_address_a_duplicate_signer_in_the_list_does_not_count_twice() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    // Same signer named twice - must not satisfy a threshold of 2.
    let duplicated = subset(&h.env, &h.signers, &[0, 0]);

    let res = h
        .client
        .try_set_settlement_address(&duplicated, &biller, &settlement);
    assert_eq!(res, Err(Ok(Error::ThresholdNotMet)));
}

#[test]
fn set_settlement_address_rejects_a_non_configured_signer() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    let outsider = Address::generate(&h.env); // never part of h.signers

    let mut attempt: Vec<Address> = Vec::new(&h.env);
    attempt.push_back(h.signers.get(0).unwrap());
    attempt.push_back(outsider);

    let res = h
        .client
        .try_set_settlement_address(&attempt, &biller, &settlement);
    assert_eq!(
        res,
        Err(Ok(Error::NotAnAdminSigner)),
        "a non-configured address in the list must hard-reject the call, even though the other \
         entry (h.signers[0]) is a real signer - never silently fall back to counting only the \
         valid ones"
    );
    assert_eq!(h.client.try_get_settlement_address(&biller), Err(Ok(Error::UnknownBiller)));
}

#[test]
fn set_settlement_address_requires_each_counted_signers_own_auth() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    let two = quorum(&h);

    h.client.set_settlement_address(&two, &biller, &settlement);

    // Both quorum signers must appear as having authorized this exact call
    // - require_auth was genuinely invoked per-signer, not short-circuited.
    let auths = h.env.auths();
    assert_eq!(auths.len(), 2, "exactly the 2 quorum signers should have been asked to authorize");
    let authorized: std::vec::Vec<Address> = auths.iter().map(|(addr, _)| addr.clone()).collect();
    assert!(authorized.contains(&h.signers.get(0).unwrap()));
    assert!(authorized.contains(&h.signers.get(1).unwrap()));
    for (_, invocation) in auths.iter() {
        match &invocation.function {
            AuthorizedFunction::Contract((contract, fn_name, _args)) => {
                assert_eq!(contract, &h.contract_id);
                assert_eq!(fn_name, &Symbol::new(&h.env, "set_settlement_address"));
            }
            other => panic!("expected a contract auth entry, got {other:?}"),
        }
    }
}

#[test]
fn get_settlement_address_for_unknown_biller_is_an_error() {
    let h = setup();
    let biller = Symbol::new(&h.env, "NOBODY");

    let res = h.client.try_get_settlement_address(&biller);
    assert_eq!(res, Err(Ok(Error::UnknownBiller)));
}

// ---------- pay: unaffected by the admin-threshold change ----------

#[test]
fn pay_to_known_biller_moves_funds_payer_to_settlement_in_one_transfer() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    h.client.set_settlement_address(&quorum(&h), &biller, &settlement);

    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);

    h.client
        .pay(&payer, &h.token, &300i128, &biller, &order_ref(&h.env, 1));

    let token = TokenClient::new(&h.env, &h.token);
    assert_eq!(token.balance(&payer), 700);
    assert_eq!(token.balance(&settlement), 300);
    // Never rests in the contract's own balance, not even momentarily.
    assert_eq!(token.balance(&h.contract_id), 0);
}

#[test]
fn pay_to_unknown_biller_is_rejected_and_moves_nothing() {
    let h = setup();
    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);
    let biller = Symbol::new(&h.env, "NOBODY");

    let res = h
        .client
        .try_pay(&payer, &h.token, &300i128, &biller, &order_ref(&h.env, 1));
    assert_eq!(res, Err(Ok(Error::UnknownBiller)));

    let token = TokenClient::new(&h.env, &h.token);
    assert_eq!(token.balance(&payer), 1_000);
}

#[test]
fn pay_rejects_zero_amount() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    h.client
        .set_settlement_address(&quorum(&h), &biller, &Address::generate(&h.env));
    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);

    let res = h
        .client
        .try_pay(&payer, &h.token, &0i128, &biller, &order_ref(&h.env, 1));
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn pay_rejects_negative_amount() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    h.client
        .set_settlement_address(&quorum(&h), &biller, &Address::generate(&h.env));
    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);

    let res = h
        .client
        .try_pay(&payer, &h.token, &-50i128, &biller, &order_ref(&h.env, 1));
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn pay_requires_the_named_payers_own_auth() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    h.client.set_settlement_address(&quorum(&h), &biller, &settlement);

    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);
    let ref_ = order_ref(&h.env, 7);

    h.client.pay(&payer, &h.token, &300i128, &biller, &ref_);

    // The invocation tree must require the named payer's own auth for this
    // exact call - not the contract's, not some other address's. This is
    // what actually makes payer impersonation impossible: a caller naming
    // someone else as `payer` cannot produce a valid signature for them.
    let invocation = &h.env.auths()[0];
    assert_eq!(
        invocation.0, payer,
        "pay must require the named payer's own auth, not an arbitrary caller's"
    );
    match &invocation.1.function {
        AuthorizedFunction::Contract((contract, fn_name, _args)) => {
            assert_eq!(contract, &h.contract_id);
            assert_eq!(fn_name, &Symbol::new(&h.env, "pay"));
        }
        other => panic!("expected a contract auth entry, got {other:?}"),
    }
}

#[test]
fn pay_cannot_be_called_without_the_real_payers_own_auth() {
    // Deliberately does NOT call env.mock_all_auths() - only the exact
    // address/invocation pairs mocked below are treated as authorized.
    let env = Env::default();

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));

    let token_admin_addr = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin_addr.clone());
    let token = token_contract.address();

    let real_payer = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Minting authority is irrelevant to the property under test (whether
    // `pay` can move real_payer's funds without real_payer's own
    // signature) - mock_all_auths briefly for setup only.
    env.mock_all_auths();
    let token_admin = StellarAssetClient::new(&env, &token);
    token_admin.mint(&real_payer, &1_000i128);

    let contract_id = env.register(BillPaymentRouter, (signers.clone(), THRESHOLD));
    let client = BillPaymentRouterClient::new(&env, &contract_id);
    let biller = Symbol::new(&env, "DISCO_IKEJA");
    let settlement = Address::generate(&env);
    let mut quorum_signers: Vec<Address> = Vec::new(&env);
    quorum_signers.push_back(signers.get(0).unwrap());
    quorum_signers.push_back(signers.get(1).unwrap());
    client.set_settlement_address(&quorum_signers, &biller, &settlement);

    // Now remove the blanket mock and authorize ONLY the attacker for THIS
    // invocation (attacker signing their own transaction, but naming
    // real_payer as the `payer` argument - exactly the impersonation this
    // contract must prevent).
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pay",
            args: (
                real_payer.clone(),
                token.clone(),
                300i128,
                biller.clone(),
                order_ref(&env, 9),
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.pay(&real_payer, &token, &300i128, &biller, &order_ref(&env, 9));
    }));
    assert!(
        result.is_err(),
        "pay must panic when the named payer (real_payer) never actually authorized this call, \
         even though some other address (attacker) is calling it"
    );

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(
        token_client.balance(&real_payer),
        1_000,
        "no funds should have moved - the unauthorized pay must not have any effect"
    );
}

// ---------- events ----------

#[test]
fn bill_paid_event_emitted_with_correct_shape_on_success() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    h.client.set_settlement_address(&quorum(&h), &biller, &settlement);

    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);
    let ref_ = order_ref(&h.env, 42);

    h.client.pay(&payer, &h.token, &300i128, &biller, &ref_);

    let expected = BillPaid {
        payer: payer.clone(),
        biller_id: biller.clone(),
        order_ref: ref_.clone(),
        amount: 300,
    };

    assert_eq!(
        h.env.events().all().filter_by_contract(&h.contract_id),
        std::vec![expected.to_xdr(&h.env, &h.contract_id)],
        "the exact event shape matters - the backend indexes off this"
    );
}

#[test]
fn no_event_emitted_when_pay_is_rejected() {
    let h = setup();
    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);
    let biller = Symbol::new(&h.env, "NOBODY");

    let _ = h
        .client
        .try_pay(&payer, &h.token, &300i128, &biller, &order_ref(&h.env, 1));

    assert!(
        h.env
            .events()
            .all()
            .filter_by_contract(&h.contract_id)
            .events()
            .is_empty(),
        "a rejected pay (unknown biller) must not emit a BillPaid event"
    );
}
