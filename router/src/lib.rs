#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    BytesN, Env, Symbol, Vec,
};

mod test;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The configured admin signer set for `set_settlement_address`, and
    /// the number of them required to approve a given call. Replaces a
    /// single `Admin` key: a compromised signer can no longer redirect
    /// future payments for a biller alone - it takes `AdminThreshold`
    /// distinct configured signers co-signing the *same* transaction. Still
    /// bounded the same way the single-key version was: this can misdirect
    /// *future* `pay` calls for a biller, never move funds already paid
    /// out, and never authorize a `pay` call on a payer's behalf (that
    /// always needs the payer's own signature regardless of this).
    AdminSigners,
    AdminThreshold,
    /// biller_id -> the address a `pay` for that biller settles to.
    Settlement(Symbol),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    UnknownBiller = 3,
    /// Constructor called with admin_threshold == 0 or > admin_signers.len().
    InvalidConfig = 4,
    /// A caller-supplied signer in `set_settlement_address` isn't one of
    /// the addresses configured at `__constructor` time.
    NotAnAdminSigner = 5,
    /// Fewer than `AdminThreshold` distinct configured signers actually
    /// authorized this call.
    ThresholdNotMet = 6,
}

/// Emitted on every successful `pay`. Topics: ("bill_paid", payer, biller_id)
/// - the event name topic is the lowercase `#[contractevent(topics=...)]`
/// value, not the struct name; `#[topic]` fields follow in declaration
/// order. Data: order_ref, amount. The backend indexes this event to
/// confirm an on-chain payment landed before calling VTpass - get this
/// shape right, it's a cross-repo contract (see router/README.md).
#[contractevent]
pub struct BillPaid {
    #[topic]
    pub payer: Address,
    #[topic]
    pub biller_id: Symbol,
    pub order_ref: BytesN<32>,
    pub amount: i128,
}

#[contract]
pub struct BillPaymentRouter;

#[contractimpl]
impl BillPaymentRouter {
    /// `admin_threshold` of the `admin_signers` must co-sign (in the same
    /// transaction) any `set_settlement_address` call. No timelock: unlike
    /// jar's `execute_upgrade` (which changes contract *code* and got a
    /// 48h delay to give observers time to react to a looming change),
    /// this changes a *data mapping* with a bounded blast radius (misdirect
    /// future payments for one biller; never steal already-paid funds;
    /// never touch `pay`'s payer-auth requirement) - see router/README.md
    /// for the full reasoning on why a single-transaction multisig is
    /// judged sufficient here without also adding a delay.
    pub fn __constructor(env: Env, admin_signers: Vec<Address>, admin_threshold: u32) {
        if admin_threshold == 0 || admin_threshold > admin_signers.len() {
            panic_with_error!(&env, Error::InvalidConfig);
        }

        env.storage()
            .instance()
            .set(&DataKey::AdminSigners, &admin_signers);
        env.storage()
            .instance()
            .set(&DataKey::AdminThreshold, &admin_threshold);
    }

    /// `signers` must contain at least `AdminThreshold` distinct addresses
    /// drawn from the configured `AdminSigners` set, each of which must
    /// actually authorize this exact invocation (require_auth, checked
    /// individually - this is a single Soroban transaction with multiple
    /// independent signatures, not a multi-transaction propose/approve
    /// flow). A caller-supplied address that isn't a configured signer, or
    /// a duplicate within `signers`, never counts toward the threshold.
    pub fn set_settlement_address(
        env: Env,
        signers: Vec<Address>,
        biller_id: Symbol,
        address: Address,
    ) -> Result<(), Error> {
        let configured: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AdminSigners)
            .ok_or(Error::NotInitialized)?;
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AdminThreshold)
            .ok_or(Error::NotInitialized)?;

        let mut counted: Vec<Address> = Vec::new(&env);
        for signer in signers.iter() {
            if !configured.contains(&signer) {
                return Err(Error::NotAnAdminSigner);
            }
            if counted.contains(&signer) {
                continue; // a duplicate in the caller-supplied list never counts twice
            }
            signer.require_auth();
            counted.push_back(signer);
        }

        if counted.len() < threshold {
            return Err(Error::ThresholdNotMet);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Settlement(biller_id), &address);
        Ok(())
    }

    /// Read-only: the configured signer set and threshold, so anyone can
    /// verify on-chain who can approve a settlement-address change without
    /// trusting off-chain documentation of it.
    pub fn get_admin_config(env: Env) -> Result<(Vec<Address>, u32), Error> {
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AdminSigners)
            .ok_or(Error::NotInitialized)?;
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AdminThreshold)
            .ok_or(Error::NotInitialized)?;
        Ok((signers, threshold))
    }

    pub fn get_settlement_address(env: Env, biller_id: Symbol) -> Result<Address, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Settlement(biller_id))
            .ok_or(Error::UnknownBiller)
    }

    /// Pays a registered biller directly from the payer's own balance to the
    /// biller's settlement address, in one atomic transfer - funds never
    /// rest in this contract's own balance, even momentarily. `payer` must
    /// be the real signer of this invocation: `require_auth` here, backed by
    /// the token contract's own auth check inside `transfer`, is what makes
    /// it impossible for a caller to move someone else's funds by simply
    /// naming them as `payer` - see `pay_cannot_be_called_without_payers_own_auth`
    /// in tests, which proves this against the auth framework, not just by
    /// inspection. Entirely unaffected by the admin-signer-threshold change
    /// above - still gated only on the payer's own signature.
    pub fn pay(
        env: Env,
        payer: Address,
        token: Address,
        amount: i128,
        biller_id: Symbol,
        order_ref: BytesN<32>,
    ) -> Result<(), Error> {
        payer.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let settlement: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(biller_id.clone()))
            .ok_or(Error::UnknownBiller)?;

        let token_client = soroban_sdk::token::TokenClient::new(&env, &token);
        token_client.transfer(&payer, &settlement, &amount);

        BillPaid {
            payer,
            biller_id,
            order_ref,
            amount,
        }
        .publish(&env);

        Ok(())
    }
}
