#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, Env, Vec,
};

mod test;

/// Early-release delay: a locked jar's owner can request early release, then
/// must wait this long before the jar actually unlocks. Gives the product a
/// cooling-off window rather than instant self-service unlocking.
const EARLY_RELEASE_DELAY_SECS: u64 = 24 * 60 * 60;

/// Upgrade timelock: once enough signers have approved a proposed upgrade,
/// it still cannot execute until this long after the last approval that
/// crossed the threshold. Gives users a window to exit before new code runs.
const UPGRADE_TIMELOCK_SECS: u64 = 48 * 60 * 60;

const DAY_IN_LEDGERS: u32 = 17280;
const JAR_TTL_THRESHOLD: u32 = 30 * DAY_IN_LEDGERS;
const JAR_TTL_EXTEND_TO: u32 = 120 * DAY_IN_LEDGERS;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Day-to-day operational admin: pause/unpause, payee list. A single key
    /// is acceptable here (testnet or mainnet) because it can never move user
    /// funds and the worst it can do is freeze new deposits/payouts.
    OperAdmin,
    /// Addresses allowed to co-sign an `upgrade` (the one function that *can*
    /// change what the contract does with user funds).
    UpgradeSigners,
    UpgradeThreshold,
    PendingUpgrade,
    AllowedTokens,
    Payees,
    Paused,
    NextJarId,
    Jar(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JarRecord {
    pub owner: Address,
    pub token: Address,
    pub balance: i128,
    pub target: i128,
    pub unlock_time: u64,
    pub locked: bool,
    pub release_requested_at: Option<u64>,
    pub closed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    pub new_wasm_hash: BytesN<32>,
    pub approvals: Vec<Address>,
    /// Set once `approvals.len() >= threshold`; execution is blocked until
    /// `UPGRADE_TIMELOCK_SECS` after this timestamp.
    pub threshold_met_at: Option<u64>,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    InvalidConfig = 3,
    TokenNotAllowed = 4,
    PayeeNotAllowed = 5,
    JarNotFound = 6,
    JarClosed = 7,
    JarLocked = 8,
    JarNotEmpty = 9,
    NotJarOwner = 10,
    InsufficientBalance = 11,
    Overflow = 12,
    ContractPaused = 13,
    NotLocked = 14,
    ReleaseAlreadyRequested = 15,
    ReleaseNotRequested = 16,
    TimelockNotElapsed = 17,
    NoPendingUpgrade = 18,
    NotAnUpgradeSigner = 19,
    AlreadyApproved = 20,
    ThresholdNotMet = 21,
}

#[contractevent]
pub struct JarCreated {
    #[topic]
    pub owner: Address,
    pub jar_id: u64,
    pub token: Address,
    pub target: i128,
    pub unlock_time: u64,
    pub locked: bool,
}

#[contractevent]
pub struct Deposited {
    #[topic]
    pub jar_id: u64,
    pub amount: i128,
    pub new_balance: i128,
}

#[contractevent]
pub struct Withdrawn {
    #[topic]
    pub jar_id: u64,
    pub amount: i128,
    pub new_balance: i128,
}

#[contractevent]
pub struct PaidOut {
    #[topic]
    pub jar_id: u64,
    #[topic]
    pub payee: Address,
    pub amount: i128,
    pub order_ref: Bytes,
}

#[contractevent]
pub struct ReleaseRequested {
    #[topic]
    pub jar_id: u64,
    pub available_at: u64,
}

#[contractevent]
pub struct Released {
    #[topic]
    pub jar_id: u64,
}

#[contractevent]
pub struct JarClosed {
    #[topic]
    pub jar_id: u64,
}

#[contractevent]
pub struct PausedEvent {
    pub paused: bool,
}

#[contract]
pub struct JarContract;

#[contractimpl]
impl JarContract {
    pub fn __constructor(
        env: Env,
        oper_admin: Address,
        upgrade_signers: Vec<Address>,
        upgrade_threshold: u32,
        allowed_tokens: Vec<Address>,
        payees: Vec<Address>,
    ) {
        if upgrade_threshold == 0 || upgrade_threshold > upgrade_signers.len() {
            panic_with_error!(&env, Error::InvalidConfig);
        }

        env.storage().instance().set(&DataKey::OperAdmin, &oper_admin);
        env.storage()
            .instance()
            .set(&DataKey::UpgradeSigners, &upgrade_signers);
        env.storage()
            .instance()
            .set(&DataKey::UpgradeThreshold, &upgrade_threshold);
        env.storage()
            .instance()
            .set(&DataKey::AllowedTokens, &allowed_tokens);
        env.storage().instance().set(&DataKey::Payees, &payees);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::NextJarId, &0u64);

        env.storage().instance().extend_ttl(JAR_TTL_THRESHOLD, JAR_TTL_EXTEND_TO);
    }

    // ---- Jars ----

    pub fn create_jar(
        env: Env,
        owner: Address,
        token: Address,
        target: i128,
        unlock_time: u64,
        locked: bool,
    ) -> Result<u64, Error> {
        Self::require_not_paused(&env)?;
        owner.require_auth();

        if target <= 0 {
            return Err(Error::InvalidAmount);
        }

        let allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .ok_or(Error::NotInitialized)?;
        if !allowed_tokens.contains(&token) {
            return Err(Error::TokenNotAllowed);
        }

        let jar_id: u64 = env.storage().instance().get(&DataKey::NextJarId).unwrap_or(0);
        let next_id = jar_id.checked_add(1).ok_or(Error::Overflow)?;
        env.storage().instance().set(&DataKey::NextJarId, &next_id);

        let jar = JarRecord {
            owner: owner.clone(),
            token: token.clone(),
            balance: 0,
            target,
            unlock_time,
            locked,
            release_requested_at: None,
            closed: false,
        };
        Self::write_jar(&env, jar_id, &jar);

        JarCreated {
            owner,
            jar_id,
            token,
            target,
            unlock_time,
            locked,
        }
        .publish(&env);

        Ok(jar_id)
    }

    pub fn deposit(env: Env, jar_id: u64, from: Address, amount: i128) -> Result<(), Error> {
        Self::require_not_paused(&env)?;
        from.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut jar = Self::read_jar(&env, jar_id)?;
        if jar.closed {
            return Err(Error::JarClosed);
        }
        if jar.owner != from {
            return Err(Error::NotJarOwner);
        }

        let token = soroban_sdk::token::TokenClient::new(&env, &jar.token);
        token.transfer(&from, &env.current_contract_address(), &amount);

        jar.balance = jar.balance.checked_add(amount).ok_or(Error::Overflow)?;
        Self::write_jar(&env, jar_id, &jar);

        Deposited {
            jar_id,
            amount,
            new_balance: jar.balance,
        }
        .publish(&env);

        Ok(())
    }

    /// Withdraws the jar's own funds back to its owner. Deliberately exempt
    /// from the paused check: pausing must never trap a user's own money,
    /// only new deposits/payouts.
    pub fn withdraw(env: Env, jar_id: u64, amount: i128) -> Result<(), Error> {
        let mut jar = Self::read_jar(&env, jar_id)?;
        jar.owner.require_auth();

        if jar.closed {
            return Err(Error::JarClosed);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let now = env.ledger().timestamp();
        let unlocked = !jar.locked || now >= jar.unlock_time;
        if !unlocked {
            return Err(Error::JarLocked);
        }

        if amount > jar.balance {
            return Err(Error::InsufficientBalance);
        }

        let token = soroban_sdk::token::TokenClient::new(&env, &jar.token);
        token.transfer(&env.current_contract_address(), &jar.owner, &amount);

        jar.balance = jar.balance.checked_sub(amount).ok_or(Error::Overflow)?;
        Self::write_jar(&env, jar_id, &jar);

        Withdrawn {
            jar_id,
            amount,
            new_balance: jar.balance,
        }
        .publish(&env);

        Ok(())
    }

    /// Pays an allowlisted biller/partner from the jar. Allowed any time
    /// (locked or not) since this is the product's intended use of saved
    /// funds, but blocked while paused like any other money leaving the jar.
    pub fn pay_out(
        env: Env,
        jar_id: u64,
        payee: Address,
        amount: i128,
        order_ref: Bytes,
    ) -> Result<(), Error> {
        Self::require_not_paused(&env)?;

        let mut jar = Self::read_jar(&env, jar_id)?;
        jar.owner.require_auth();

        if jar.closed {
            return Err(Error::JarClosed);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let payees: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Payees)
            .ok_or(Error::NotInitialized)?;
        if !payees.contains(&payee) {
            return Err(Error::PayeeNotAllowed);
        }

        if amount > jar.balance {
            return Err(Error::InsufficientBalance);
        }

        let token = soroban_sdk::token::TokenClient::new(&env, &jar.token);
        token.transfer(&env.current_contract_address(), &payee, &amount);

        jar.balance = jar.balance.checked_sub(amount).ok_or(Error::Overflow)?;
        Self::write_jar(&env, jar_id, &jar);

        PaidOut {
            jar_id,
            payee,
            amount,
            order_ref,
        }
        .publish(&env);

        Ok(())
    }

    pub fn request_early_release(env: Env, jar_id: u64) -> Result<(), Error> {
        let mut jar = Self::read_jar(&env, jar_id)?;
        jar.owner.require_auth();

        if jar.closed {
            return Err(Error::JarClosed);
        }
        if !jar.locked {
            return Err(Error::NotLocked);
        }
        if jar.release_requested_at.is_some() {
            return Err(Error::ReleaseAlreadyRequested);
        }

        let now = env.ledger().timestamp();
        jar.release_requested_at = Some(now);
        Self::write_jar(&env, jar_id, &jar);

        ReleaseRequested {
            jar_id,
            available_at: now.saturating_add(EARLY_RELEASE_DELAY_SECS),
        }
        .publish(&env);

        Ok(())
    }

    pub fn claim_early_release(env: Env, jar_id: u64) -> Result<(), Error> {
        let mut jar = Self::read_jar(&env, jar_id)?;
        jar.owner.require_auth();

        if jar.closed {
            return Err(Error::JarClosed);
        }
        if !jar.locked {
            return Err(Error::NotLocked);
        }
        let requested_at = jar.release_requested_at.ok_or(Error::ReleaseNotRequested)?;

        let now = env.ledger().timestamp();
        if now < requested_at.saturating_add(EARLY_RELEASE_DELAY_SECS) {
            return Err(Error::TimelockNotElapsed);
        }

        jar.locked = false;
        jar.release_requested_at = None;
        Self::write_jar(&env, jar_id, &jar);

        Released { jar_id }.publish(&env);

        Ok(())
    }

    pub fn close_jar(env: Env, jar_id: u64) -> Result<(), Error> {
        let mut jar = Self::read_jar(&env, jar_id)?;
        jar.owner.require_auth();

        if jar.closed {
            return Err(Error::JarClosed);
        }
        if jar.balance != 0 {
            return Err(Error::JarNotEmpty);
        }

        jar.closed = true;
        Self::write_jar(&env, jar_id, &jar);

        JarClosed { jar_id }.publish(&env);

        Ok(())
    }

    /// Anyone may call this — extending TTL moves no funds and changes no
    /// jar state, so gating it behind auth would add friction without adding
    /// safety (any account can already bump an entry's TTL at the ledger-op
    /// level via ExtendFootprintTTLOp).
    pub fn extend_ttl(env: Env, jar_id: u64) -> Result<(), Error> {
        let _ = Self::read_jar(&env, jar_id)?;
        env.storage().persistent().extend_ttl(
            &DataKey::Jar(jar_id),
            JAR_TTL_THRESHOLD,
            JAR_TTL_EXTEND_TO,
        );
        Ok(())
    }

    // ---- Views ----

    pub fn get_jar(env: Env, jar_id: u64) -> Result<JarRecord, Error> {
        Self::read_jar(&env, jar_id)
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    pub fn get_payees(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::Payees).unwrap_or(Vec::new(&env))
    }

    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or(Vec::new(&env))
    }

    // ---- Operational admin (single key; can never move user funds) ----

    pub fn set_paused(env: Env, paused: bool) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::OperAdmin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.storage().instance().set(&DataKey::Paused, &paused);
        PausedEvent { paused }.publish(&env);
        Ok(())
    }

    pub fn set_payees(env: Env, payees: Vec<Address>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::OperAdmin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.storage().instance().set(&DataKey::Payees, &payees);
        Ok(())
    }

    // ---- Upgrade: 2-of-3 (configurable N-of-M) signers + 48h timelock.
    // No single key — including OperAdmin — can upgrade the contract alone.

    pub fn propose_upgrade(env: Env, proposer: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_upgrade_signer(&env, &proposer)?;
        proposer.require_auth();

        let mut approvals = Vec::new(&env);
        approvals.push_back(proposer);

        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeThreshold)
            .ok_or(Error::NotInitialized)?;
        let threshold_met_at = if approvals.len() >= threshold {
            Some(env.ledger().timestamp())
        } else {
            None
        };

        let pending = PendingUpgrade {
            new_wasm_hash,
            approvals,
            threshold_met_at,
        };
        env.storage().instance().set(&DataKey::PendingUpgrade, &pending);
        Ok(())
    }

    pub fn approve_upgrade(env: Env, approver: Address) -> Result<(), Error> {
        Self::require_upgrade_signer(&env, &approver)?;
        approver.require_auth();

        let mut pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(Error::NoPendingUpgrade)?;

        if pending.approvals.contains(&approver) {
            return Err(Error::AlreadyApproved);
        }
        pending.approvals.push_back(approver);

        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeThreshold)
            .ok_or(Error::NotInitialized)?;
        if pending.threshold_met_at.is_none() && pending.approvals.len() >= threshold {
            pending.threshold_met_at = Some(env.ledger().timestamp());
        }

        env.storage().instance().set(&DataKey::PendingUpgrade, &pending);
        Ok(())
    }

    pub fn cancel_upgrade(env: Env, canceller: Address) -> Result<(), Error> {
        Self::require_upgrade_signer(&env, &canceller)?;
        canceller.require_auth();

        if !env.storage().instance().has(&DataKey::PendingUpgrade) {
            return Err(Error::NoPendingUpgrade);
        }
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        Ok(())
    }

    pub fn execute_upgrade(env: Env) -> Result<(), Error> {
        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(Error::NoPendingUpgrade)?;

        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeThreshold)
            .ok_or(Error::NotInitialized)?;
        if pending.approvals.len() < threshold {
            return Err(Error::ThresholdNotMet);
        }
        let threshold_met_at = pending.threshold_met_at.ok_or(Error::ThresholdNotMet)?;

        let now = env.ledger().timestamp();
        if now < threshold_met_at.saturating_add(UPGRADE_TIMELOCK_SECS) {
            return Err(Error::TimelockNotElapsed);
        }

        env.deployer().update_current_contract_wasm(pending.new_wasm_hash);
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        Ok(())
    }

    // ---- internal ----

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn require_upgrade_signer(env: &Env, who: &Address) -> Result<(), Error> {
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeSigners)
            .ok_or(Error::NotInitialized)?;
        if !signers.contains(who) {
            return Err(Error::NotAnUpgradeSigner);
        }
        Ok(())
    }

    fn read_jar(env: &Env, jar_id: u64) -> Result<JarRecord, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Jar(jar_id))
            .ok_or(Error::JarNotFound)
    }

    fn write_jar(env: &Env, jar_id: u64, jar: &JarRecord) {
        env.storage().persistent().set(&DataKey::Jar(jar_id), jar);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Jar(jar_id), JAR_TTL_THRESHOLD, JAR_TTL_EXTEND_TO);
    }
}
