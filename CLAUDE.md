# BillSteady Contracts

Soroban contracts for BillSteady: pay real Nigerian bills directly from a
Stellar wallet, in XLM or USDC, non-custodially.

This repo is one of four:
- [billsteady-web](https://github.com/BillSteady/billsteady-web): the wallet-connect + pay-bill PWA
- [billsteady-backend](https://github.com/BillSteady/billsteady-backend): Supabase backend
- [billsteady-landing](https://github.com/BillSteady/billsteady-landing): the public landing page
- **billsteady-contracts** (this repo): Soroban contracts (Rust)

## Product pivot (2026-11-08)

BillSteady pivoted from "save daily, no crypto" to a non-custodial
Stellar/stablecoin bill-pay app. This repo now has two contracts:
- **`router/` — active.** `BillPaymentRouter`: one function, atomic
  payer-to-settlement transfer, no contract-held custody at any point, no
  upgrade machinery (there's nothing held to protect from a bad upgrade).
  See `router/README.md` for the interface, the exact `BillPaid` event
  shape (`billsteady-backend` depends on it byte-for-byte), and the
  admin-key trust-boundary decision.
- **`jar/` — archived.** The pre-pivot "save over time" contract. Real,
  tested, testnet-proven — kept as a historical record, not deleted. See
  `jar/README.md` for the superseded note and the full original detail.

## Stack
- Rust, soroban-sdk 27, Stellar CLI 27.1.0 (re-check against Protocol 29 /
  soroban-sdk 28 before any mainnet deploy — built on what was installed)
- STELLAR_NETWORK env var, default testnet

## Commands
- `cargo test --manifest-path router/Cargo.toml` (active contract)
- `cargo test --manifest-path jar/Cargo.toml` (archived, still passes)
- `stellar contract build` (run from within `router/` or `jar/`)
- `./router/scripts/testnet-demo.sh` / `./jar/scripts/testnet-demo.sh` —
  real end-to-end testnet proof, fresh throwaway identities each run

## Rules
- i128 with 7 decimals on-chain. Never floats.
- require_auth on every fund-moving call. No admin function can move a
  payer's funds without the payer's own signature.
- Checked arithmetic, no zero or negative amounts.
- A single admin key is accepted risk for v1 (can misdirect future
  payments, can never steal — see `router/README.md`'s trust-boundary
  section) — revisit with a signer threshold before real payment volume
  depends on it.

## Workflow
- Write cargo tests first for every function and edge case, then implement.
- The most important test in any fund-moving function is proving an
  unauthorized caller genuinely cannot move someone else's funds — see
  `router/scripts/testnet-demo.sh`'s documented methodology fix (the
  Stellar CLI auto-signs with any locally-available key, which can produce
  a false-positive "secure" result unless you deliberately remove the
  payer's key before testing the attacker path for real).
- External audit required before any mainnet deploy — no exceptions.

## Current phase
`router/`: written, 11/11 tests passing, proven end-to-end on Stellar
testnet (real tx hashes, independently confirmed on Horizon, including the
real negative-auth-case proof). UNAUDITED, UNDEPLOYED to mainnet. No
static analysis or fuzz testing run on it yet (see issues #2/#3's history
on `jar/` for what that process looks like in this environment, and
`router/README.md`'s "What was not verified").

`jar/`: archived but intact — 45/45 tests, testnet-proven, same
unaudited/undeployed status. Not part of the active product.
