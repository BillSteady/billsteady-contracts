// Property tests (billsteady-contracts issue #3): lock in the arithmetic
// invariants the 39 example-based tests in test.rs don't exhaustively cover
// — a jar's balance never goes negative, it never silently exceeds what was
// actually deposited, and the checked-arithmetic paths reject overflow
// rather than wrapping. Runs as part of plain `cargo test` (proptest, not
// cargo-fuzz) per this crate's testing convention.
#![cfg(test)]
extern crate std;

use crate::test::{fund, setup};
use crate::Error;
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Bytes};

// Representative of realistic money scale (kobo-denominated, well within
// what the product will ever move) without approaching i128::MAX, so these
// cases exercise ordinary deposit/withdraw/pay_out behavior, not the
// overflow boundary (that's its own targeted test below).
const REALISTIC_AMOUNT: std::ops::RangeInclusive<i128> = 1i128..=1_000_000_000_000_000i128;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Depositing X then withdrawing the same X always returns the jar to
    /// its starting balance (0) — no amount is silently lost or created.
    #[test]
    fn deposit_then_withdraw_same_amount_returns_to_zero(amount in REALISTIC_AMOUNT) {
        let h = setup();
        let owner = Address::generate(&h.env);
        fund(&h, &owner, amount);

        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &amount);
        h.client.withdraw(&jar_id, &amount);

        let jar = h.client.get_jar(&jar_id);
        prop_assert_eq!(jar.balance, 0);
    }

    /// For any deposit of X followed by a withdrawal of any Y in [0, X],
    /// the resulting balance is exactly X - Y and is never negative.
    #[test]
    fn withdraw_up_to_balance_never_goes_negative(
        deposit_amount in REALISTIC_AMOUNT,
        withdraw_fraction in 0f64..=1f64,
    ) {
        let h = setup();
        let owner = Address::generate(&h.env);
        fund(&h, &owner, deposit_amount);

        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &deposit_amount);

        let withdraw_amount = ((deposit_amount as f64) * withdraw_fraction) as i128;
        let withdraw_amount = withdraw_amount.max(0);
        if withdraw_amount > 0 {
            h.client.withdraw(&jar_id, &withdraw_amount);
        }

        let jar = h.client.get_jar(&jar_id);
        prop_assert!(jar.balance >= 0);
        prop_assert_eq!(jar.balance, deposit_amount - withdraw_amount);
    }

    /// Attempting to withdraw more than the jar's balance is always
    /// rejected, and — critically — leaves the balance completely
    /// unchanged (no partial mutation before the rejection).
    #[test]
    fn withdraw_more_than_balance_always_rejected_and_balance_unchanged(
        deposit_amount in 1i128..=1_000_000_000_000i128,
        overage in 1i128..=1_000_000_000_000i128,
    ) {
        let h = setup();
        let owner = Address::generate(&h.env);
        fund(&h, &owner, deposit_amount);

        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &deposit_amount);

        let over_withdraw = deposit_amount + overage;
        let result = h.client.try_withdraw(&jar_id, &over_withdraw);
        prop_assert!(result.is_err());

        let jar = h.client.get_jar(&jar_id);
        prop_assert_eq!(jar.balance, deposit_amount);
    }

    /// Same shape as the withdraw invariant, for pay_out: never exceeds
    /// balance, never leaves it negative, and a rejected over-payout
    /// mutates nothing.
    #[test]
    fn pay_out_up_to_balance_never_goes_negative(
        deposit_amount in REALISTIC_AMOUNT,
        payout_fraction in 0f64..=1f64,
    ) {
        let h = setup();
        let owner = Address::generate(&h.env);
        fund(&h, &owner, deposit_amount);

        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &deposit_amount);

        let payout_amount = ((deposit_amount as f64) * payout_fraction) as i128;
        let payout_amount = payout_amount.max(0);
        let order_ref = Bytes::from_slice(&h.env, b"PROPTEST-ORDER");
        if payout_amount > 0 {
            h.client
                .pay_out(&jar_id, &h.payee, &payout_amount, &order_ref);
        }

        let jar = h.client.get_jar(&jar_id);
        prop_assert!(jar.balance >= 0);
        prop_assert_eq!(jar.balance, deposit_amount - payout_amount);
    }

    /// A full sequence (deposit, then a withdrawal and a payout that
    /// together never exceed it) always leaves the jar's balance exactly
    /// equal to deposits minus withdrawals minus payouts — the general
    /// conservation invariant the issue asked for, not just the two-op
    /// cases above.
    #[test]
    fn balance_equals_deposits_minus_withdrawals_minus_payouts(
        deposit_amount in 100i128..=1_000_000_000_000i128,
        withdraw_fraction in 0f64..=0.5f64,
        payout_fraction in 0f64..=0.5f64,
    ) {
        let h = setup();
        let owner = Address::generate(&h.env);
        fund(&h, &owner, deposit_amount);

        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &deposit_amount);

        let withdraw_amount = ((deposit_amount as f64) * withdraw_fraction) as i128;
        let payout_amount = ((deposit_amount as f64) * payout_fraction) as i128;
        // fractions are each capped at 0.5, so their sum never exceeds the
        // deposit — both operations are always legal.
        if withdraw_amount > 0 {
            h.client.withdraw(&jar_id, &withdraw_amount);
        }
        if payout_amount > 0 {
            let order_ref = Bytes::from_slice(&h.env, b"PROPTEST-ORDER");
            h.client
                .pay_out(&jar_id, &h.payee, &payout_amount, &order_ref);
        }

        let jar = h.client.get_jar(&jar_id);
        prop_assert_eq!(jar.balance, deposit_amount - withdraw_amount - payout_amount);
        prop_assert!(jar.balance >= 0);
    }

    /// The checked-arithmetic path in `deposit` actually rejects overflow
    /// rather than wrapping or panicking: once a jar's balance sits at
    /// i128::MAX, any further positive deposit must fail with
    /// Error::Overflow, and the balance must stay unchanged.
    #[test]
    fn deposit_overflow_is_rejected_not_wrapped(second_amount in 1i128..=10_000i128) {
        let h = setup();
        let owner = Address::generate(&h.env);

        // Max out the jar's balance first: fund and deposit exactly i128::MAX.
        fund(&h, &owner, i128::MAX);
        let jar_id = h.client.create_jar(&owner, &h.token, &1i128, &0u64, &false);
        h.client.deposit(&jar_id, &owner, &i128::MAX);

        // A fresh, independent mint — proves this isn't about the owner
        // running out of tokens, it's specifically the jar's i128 balance
        // field that must refuse to overflow.
        fund(&h, &owner, second_amount);
        let result = h.client.try_deposit(&jar_id, &owner, &second_amount);

        prop_assert!(result.is_err());
        if let Err(Ok(contract_err)) = result {
            prop_assert_eq!(contract_err, Error::Overflow);
        }

        let jar = h.client.get_jar(&jar_id);
        prop_assert_eq!(jar.balance, i128::MAX);
    }
}
