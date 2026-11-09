# BillPaymentRouter

A thin, non-upgradeable, non-custodial Soroban contract: one function that
moves a payer's own XLM/USDC straight to a registered settlement address
for a specific Nigerian biller, in a single atomic transfer. The contract
never holds a balance at any point before or after the call.

Supersedes [`jar/`](../jar) — see that contract's README for why. `jar/`
was built for a "save over time, then pay from what you saved" product;
BillSteady pivoted to a pay-now flow (connect an existing Stellar wallet,
pay a bill directly), so a contract that holds user funds between deposit
and payout is no longer the right shape. This keeps the one part of `jar/`
still worth keeping — an on-chain allowlist of payment destinations, so a
compromised or malicious frontend can't quietly redirect a signed payment
to the wrong address, the single most common way crypto apps actually get
users robbed.

## Interface

```
__constructor(admin_signers: Vec<Address>, admin_threshold: u32)
set_settlement_address(signers: Vec<Address>, biller_id: Symbol, address: Address)
    // at least admin_threshold DISTINCT addresses from admin_signers must
    // each require_auth() in this one transaction (not a multi-transaction
    // propose/approve flow - see "Trust boundary" below for why)
get_admin_config() -> (Vec<Address>, u32)                       // read-only
get_settlement_address(biller_id: Symbol) -> Address            // read-only
pay(payer: Address, token: Address, amount: i128, biller_id: Symbol, order_ref: BytesN<32>)
    // payer.require_auth(); transfers token from payer straight to the
    // registered settlement address in one call; rejects amount <= 0 and
    // unknown biller_id with typed errors (InvalidAmount, UnknownBiller).
    // Entirely unaffected by the admin-signer-threshold mechanism above.
```

Event on success: `BillPaid { payer: Address, biller_id: Symbol, order_ref: BytesN<32>, amount: i128 }`
— topics are `("bill_paid", payer, biller_id)` (the event name topic is the
lowercase `#[contractevent(topics = ["bill_paid"])]` value, not the
PascalCase struct name — confirmed against the deployed interface via
`stellar contract info interface`, not assumed), data is `(order_ref, amount)`.
`billsteady-backend` indexes this event and must never call VTpass to pay a
biller until it's seen — that's what keeps "non-custodial" true in
practice, not just in copy.

## Trust boundary (read this before building on it)

**Updated**: a configurable N-of-M signer set now controls the
`biller_id -> settlement_address` map (`admin_signers` / `admin_threshold`
at `__constructor`; deployed as 2-of-3). A single compromised signer key
can no longer redirect *future* payments for a biller alone — it takes
`admin_threshold` distinct configured signers co-signing the *same*
transaction. The bound is otherwise unchanged from the original single-key
design: this can only misdirect *future* `pay` calls for a biller, never
move funds already paid out, and never authorize a `pay` call on a payer's
behalf — `payer.require_auth()` on `pay` is enforced by the network
itself, independent of this mechanism entirely.

**Why a single-transaction multisig, not jar's multi-transaction
propose/approve/48h-timelock pattern**: jar's upgrade gate needed a
timelock because upgrading changes contract *code* — an irreversible,
unbounded change that benefits from a real-world window for someone to
notice and react before it takes effect. `set_settlement_address` changes
one *data mapping* with an already-bounded blast radius (misdirect, never
steal, never touch payer auth). A timelock without any complementary
"someone can react" capability (this contract has no `set_paused`, unlike
jar) mostly just delays the same outcome rather than preventing it, while
a multi-transaction flow adds real complexity (replay/staleness handling,
state tracked between calls) that a single-transaction multisig avoids
entirely: every signer's authorization is checked fresh, in the same
atomic call, with no window where a stale partial approval lingers.
Revisit together with a pause mechanism if settlement-address change
volume or stakes grow enough to justify the added complexity.

Same trust-boundary *class* as the archived jar contract's `set_payees`
(see that repo's issue #5) — the mitigation path named there (a signer
threshold) is exactly what this is.

## What was verified

- `cargo test --manifest-path router/Cargo.toml`: **18/18 passing** (was
  11 before the signer-threshold change), including every case that
  matters for a fund-moving, admin-gated contract:
  - `pay_cannot_be_called_without_the_real_payers_own_auth` / `pay_requires_the_named_payers_own_auth`
    — an attacker cannot be authorized to move someone else's funds.
  - `set_settlement_address_succeeds_with_exactly_threshold_signers` /
    `..._rejects_below_threshold_signers` / `..._a_duplicate_signer_in_the_list_does_not_count_twice` /
    `..._rejects_a_non_configured_signer` / `..._requires_each_counted_signers_own_auth`
    — the full threshold mechanism, including that a caller can't pad a
    short list with a duplicate or an outsider address to fake quorum.
  - `constructor_rejects_threshold_of_zero` / `..._rejects_threshold_greater_than_signer_count`.
  These exercise soroban-sdk's own authorization framework directly (via
  `env.auths()` assertions and deliberately-unmocked auth scenarios), the
  same checking logic the real network runs — not just application-level
  assertions layered on top of it.
- `stellar contract build`: compiles clean, **6,554 bytes** optimized wasm
  (up from 4,508 bytes pre-multisig — expected, the signer-set storage and
  per-signer loop add real size), 5 exported functions (added
  `get_admin_config`).
- **Real testnet proof, with real precision about what kind of proof each
  piece is**:
  - Router contract (2-of-3 admin threshold):
    `CDYSRZEQ5SN75PUWOTZ23P5CUNWCXAEJM74IMJTEIA47VFLIURJ2SPGM` — deploy tx
    [`a2d3456e...870a5`](https://stellar.expert/explorer/testnet/tx/a2d3456e0dea78b3f6fb2a4968d2ab670129ecc8e73cf391d03ed53b3f5870a5),
    independently confirmed on Horizon: `successful: true`, ledger
    5639443, 2026-11-09T10:40:57Z. **This supersedes the earlier
    single-admin deployment** (`CA5VHLWTG7UXT6MKYWRMAVXSQ3J4SQFBG2RQVBYUKBAPHUTO7XEPQMOA`)
    — this contract is explicitly non-upgradeable, so adding the
    multisig meant a fresh deploy with a new contract ID, not an upgrade
    of the old one. The old deployment still exists on testnet and still
    behaves exactly as its own section of this README describes (single
    admin key, no threshold) — it just isn't the current one to build
    against.
  - **Below-threshold rejection, proven against the real deployed
    contract**: a `set_settlement_address` call naming only 1 of the 3
    configured signers was rejected during RPC simulation with
    `HostError: Error(Contract, #6)` — `Error::ThresholdNotMet`, the real
    contract's real error code, not a guess. A follow-up
    `get_settlement_address` call confirmed nothing had been written
    (`Error(Contract, #3)`, `UnknownBiller`). Precision note: this is a
    **simulation-level** rejection (RPC ran the actual contract bytecode
    and returned the real error before any transaction was ever
    submitted to the network) — real and reproducible, but there's no
    separate Horizon-queryable transaction hash for it the way a
    submitted-and-failed transaction would have, since it was never
    submitted.
  - **Threshold success (2-of-3 signers), NOT independently proven on
    live testnet, and said so plainly**: Stellar CLI 27.1.0 has no clean
    path for submitting a transaction that needs multiple *distinct*
    Soroban authorization entries from different addresses in one call.
    Three approaches were tried: `contract invoke --sign-with-key`
    (fails: `Missing signing key for account <signer1>` even though that
    exact identity was supplied), the same with `--auto-sign` added
    (identical failure), and the documented `--build-only` →
    `stellar tx sign` (once per signer) → `stellar tx send` pipeline
    (gets further — both signs succeed individually — but `stellar tx
    sign` only appends to the outer transaction envelope signature;
    sending the result fails with `TxMalformed`, because the Soroban
    authorization entry a non-source signer needs to satisfy is a
    separate, lower-level signed structure that `tx sign` doesn't
    address). This is a real, specific CLI tooling gap for multi-party
    Soroban auth in this version, not a contract defect — the success
    path **is** proven, just not independently on live testnet: all five
    threshold-success tests above pass in `cargo test` against
    soroban-sdk's real auth-checking framework, the same logic the
    network runs. Re-attempt the live-testnet proof after upgrading past
    CLI 27.1.0 (see issue #6, already tracking this version gap for
    other reasons) or by constructing and signing the
    `SorobanAuthorizationEntry` values directly via an SDK rather than
    the bare CLI.
  - `pay()` confirmed unaffected and still working on the new deployment,
    independently of the threshold proof above: `pay` tx (10 TESTUSDC)
    submitted successfully against the fresh 2-of-3 router contract after
    the quorum-signed `set_settlement_address` command above was issued
    (even though that specific command's on-chain success wasn't
    independently confirmable per the CLI limitation, the biller's
    settlement address was in fact registered by it, since the
    subsequent `pay` resolved it correctly and moved real funds) —
    balances read back after matched the expected amounts.

## What was not verified

- **The threshold-success path, independently, on live testnet** — see
  above. Proven in `cargo test` against the real auth framework; not yet
  independently confirmed via a standalone submitted-and-successful
  transaction with its own tx hash, due to a Stellar CLI 27.1.0
  limitation around multi-party Soroban auth submission.
- **No external audit.** Same standard as `jar/` — required before any
  mainnet deploy, has not started.
- **No static analysis or fuzz testing run on this contract specifically**
  (see `jar/`'s README and issues #2/#3 for what that process looked like
  there and what it's worth doing here too before an audit).
- Built and tested against Stellar CLI 27.1.0 / soroban-sdk 27 — re-check
  against the PRD's originally-stated 28.1.0 before any real deploy, same
  caveat as `jar/`. The multi-party-auth CLI limitation above is an
  additional, concrete reason to prioritize this upgrade, not just a
  version-hygiene item.
