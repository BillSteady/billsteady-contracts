#![cfg(test)]
extern crate std;

use crate::{BillPaid, BillPaymentRouter, BillPaymentRouterClient, Error};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Events, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env, Event, IntoVal, Symbol,
};

struct Harness<'a> {
    env: Env,
    client: BillPaymentRouterClient<'a>,
    contract_id: Address,
    token: Address,
    token_admin: StellarAssetClient<'a>,
    admin: Address,
}

fn setup() -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token_admin_addr = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin_addr);
    let token = token_contract.address();
    let token_admin = StellarAssetClient::new(&env, &token);

    let contract_id = env.register(BillPaymentRouter, (admin.clone(),));
    let client = BillPaymentRouterClient::new(&env, &contract_id);

    Harness {
        env,
        client,
        contract_id,
        token,
        token_admin,
        admin,
    }
}

fn fund(h: &Harness, who: &Address, amount: i128) {
    h.token_admin.mint(who, &amount);
}

fn order_ref(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// ---------- settlement address admin ----------

#[test]
fn admin_can_set_and_read_settlement_address() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);

    h.client.set_settlement_address(&biller, &settlement);

    assert_eq!(h.client.get_settlement_address(&biller), settlement);
}

#[test]
fn set_settlement_address_requires_admin_auth() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);

    h.client.set_settlement_address(&biller, &settlement);

    let invocation = &h.env.auths()[0];
    assert_eq!(
        invocation.0, h.admin,
        "set_settlement_address must require the admin's auth, not an arbitrary caller"
    );
}

#[test]
fn get_settlement_address_for_unknown_biller_is_an_error() {
    let h = setup();
    let biller = Symbol::new(&h.env, "NOBODY");

    let res = h.client.try_get_settlement_address(&biller);
    assert_eq!(res, Err(Ok(Error::UnknownBiller)));
}

// ---------- pay ----------

#[test]
fn pay_to_known_biller_moves_funds_payer_to_settlement_in_one_transfer() {
    let h = setup();
    let biller = Symbol::new(&h.env, "DISCO_IKEJA");
    let settlement = Address::generate(&h.env);
    h.client.set_settlement_address(&biller, &settlement);

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
        .set_settlement_address(&biller, &Address::generate(&h.env));
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
        .set_settlement_address(&biller, &Address::generate(&h.env));
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
    h.client.set_settlement_address(&biller, &settlement);

    let payer = Address::generate(&h.env);
    fund(&h, &payer, 1_000);
    let ref_ = order_ref(&h.env, 7);

    h.client
        .pay(&payer, &h.token, &300i128, &biller, &ref_);

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

    let admin = Address::generate(&env);
    let token_admin_addr = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin_addr.clone());
    let token = token_contract.address();

    // Minting needs the token admin's auth - mock that one call only.
    env.mock_auths(&[MockAuth {
        address: &token_admin_addr,
        invoke: &MockAuthInvoke {
            contract: &token,
            fn_name: "mint",
            args: (Address::generate(&env),).into_val(&env), // placeholder, replaced below
            sub_invokes: &[],
        },
    }]);

    let real_payer = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Mint to real_payer using mock_all_auths briefly is simpler and doesn't
    // weaken the actual test below (minting authority is irrelevant to the
    // property under test: whether `pay` can move real_payer's funds
    // without real_payer's own signature).
    env.mock_all_auths();
    let token_admin = StellarAssetClient::new(&env, &token);
    token_admin.mint(&real_payer, &1_000i128);

    let contract_id = env.register(BillPaymentRouter, (admin.clone(),));
    let client = BillPaymentRouterClient::new(&env, &contract_id);
    let biller = Symbol::new(&env, "DISCO_IKEJA");
    let settlement = Address::generate(&env);
    client.set_settlement_address(&biller, &settlement);

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
    h.client.set_settlement_address(&biller, &settlement);

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
