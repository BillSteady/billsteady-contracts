#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, BytesN, Env, Symbol,
};

mod test;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Controls which address a `biller_id` settles to. A compromised admin
    /// can redirect *future* payments for a biller to an attacker address -
    /// it can never move funds that have already been paid out, and it can
    /// never authorize a `pay` call on a payer's behalf (that still needs
    /// the payer's own signature). Single key for v1, same trust-boundary
    /// class and same accepted-risk reasoning as the jar contract's
    /// OperAdmin/set_payees (see that repo's issue #5): this key can misdirect,
    /// never steal. Revisit (e.g. a signer threshold, same pattern as the
    /// jar's upgrade gate) before real payment volume depends on it - not
    /// built in from day one because there is nothing here yet to protect
    /// against a bad actor other than this exact scenario, and the jar's
    /// 2-of-3+timelock machinery is real complexity this contract was
    /// explicitly built to avoid.
    Admin,
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
}

/// Emitted on every successful `pay`. Topics: ("BillPaid", payer, biller_id) -
/// the event name is the first topic automatically (soroban-sdk's
/// `#[contractevent]` derives it from the struct name), `#[topic]` fields
/// follow in declaration order. Data: order_ref, amount. The backend
/// indexes this event to confirm an on-chain payment landed before calling
/// VTpass - get this shape right, it's a cross-repo contract.
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
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Admin-only. See the `DataKey::Admin` doc comment for the trust
    /// boundary this creates.
    pub fn set_settlement_address(
        env: Env,
        biller_id: Symbol,
        address: Address,
    ) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .persistent()
            .set(&DataKey::Settlement(biller_id), &address);
        Ok(())
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
    /// inspection.
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
