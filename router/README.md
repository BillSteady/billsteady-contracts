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
__constructor(admin: Address)
set_settlement_address(biller_id: Symbol, address: Address)   // admin.require_auth()
get_settlement_address(biller_id: Symbol) -> Address            // read-only
pay(payer: Address, token: Address, amount: i128, biller_id: Symbol, order_ref: BytesN<32>)
    // payer.require_auth(); transfers token from payer straight to the
    // registered settlement address in one call; rejects amount <= 0 and
    // unknown biller_id with typed errors (InvalidAmount, UnknownBiller).
```

Event on success: `BillPaid { payer: Address, biller_id: Symbol, order_ref: BytesN<32>, amount: i128 }`
— topics are `("BillPaid", payer, biller_id)`, data is `(order_ref, amount)`.
`billsteady-backend` indexes this event and must never call VTpass to pay a
biller until it's seen — that's what keeps "non-custodial" true in
practice, not just in copy.

## Trust boundary (read this before building on it)

A single admin key controls the `biller_id -> settlement_address` map. A
compromised admin key can redirect *future* payments for a biller to an
attacker's address. It **cannot** steal funds already paid out, and it
**cannot** authorize a `pay` call on anyone's behalf — `payer.require_auth()`
is enforced by the network itself, not by this contract trusting the admin.
This is the same trust-boundary class as the archived jar contract's
`set_payees` (see that repo's issue #5), same accepted-risk reasoning:
revisit with a signer threshold before real payment volume depends on it.
Not built in from day one deliberately — there's nothing here to protect
beyond this one scenario, and the jar's 2-of-3-signer + 48h-timelock
upgrade machinery is exactly the complexity this contract exists to avoid.

## What was verified

- `cargo test --manifest-path router/Cargo.toml`: **11/11 passing**,
  including `pay_cannot_be_called_without_the_real_payers_own_auth` (an
  attacker cannot be authorized to move someone else's funds) and
  `pay_requires_the_named_payers_own_auth` (the payer argument itself must
  be the one who signed).
- `stellar contract build`: compiles clean, **4,508 bytes** optimized wasm,
  4 exported functions.
- **Real testnet run** (`router/scripts/testnet-demo.sh`), independently
  confirmed on Horizon, not just "CLI said success":
  - Router contract: `CA5VHLWTG7UXT6MKYWRMAVXSQ3J4SQFBG2RQVBYUKBAPHUTO7XEPQMOA`
  - Test token (SAC-wrapped TESTUSDC): `CANZP36EZM5PQ3HZ6KOBO4QNN7OZCXED6Z4B45BGFFKSS5WQGBCFJRA2`
  - `pay` tx (24 TESTUSDC, payer → settlement):
    [`5044df9c29cd084940cf0fb5bfd26aeb52d77755acd17d9843c81eadfd0de15b`](https://stellar.expert/explorer/testnet/tx/5044df9c29cd084940cf0fb5bfd26aeb52d77755acd17d9843c81eadfd0de15b) —
    confirmed on Horizon: `successful: true`, ledger 5636259,
    2026-11-09T06:15:37Z. Payer and settlement balances read back after the
    run matched exactly (999,760,000,000 and 240,000,000 respectively, at
    7 decimals).
  - **Negative case, proven on the real network, not just in the simulator**:
    an attacker account with no access to the payer's key attempted `pay`
    naming the real payer as `payer`. The script deliberately removes the
    payer's key from the local keystore first — an earlier version of this
    test left both keys available locally, which let the Stellar CLI
    silently auto-sign as the payer on the attacker's behalf, producing a
    false-positive "it's secure" result for the wrong reason. Forcing the
    real condition (payer's key genuinely unavailable), the CLI can't even
    construct a valid authorization (`Missing signing key`, exit 1) — proof
    that no path exists to forge payer authorization without their real
    key.

## What was not verified

- **No external audit.** Same standard as `jar/` — required before any
  mainnet deploy, has not started.
- **No static analysis or fuzz testing run on this contract specifically**
  (see `jar/`'s README and issues #2/#3 for what that process looked like
  there and what it's worth doing here too before an audit).
- Built and tested against Stellar CLI 27.1.0 / soroban-sdk 27 — re-check
  against the PRD's originally-stated 28.1.0 before any real deploy, same
  caveat as `jar/`.
