# BillSteady Contracts

The Soroban jar contract for BillSteady. Non-custodial: the user's wallet
signs every deposit/withdraw/pay_out. No admin function may move user funds.

This repo is one of three:
- [billsteady-web](https://github.com/BillSteady/billsteady-web): the planner PWA
- [billsteady-backend](https://github.com/BillSteady/billsteady-backend): Supabase backend
- **billsteady-contracts** (this repo): the Soroban jar contract (Rust)

## Stack
- Rust, soroban-sdk 28.0.0, Stellar CLI 28.1.0 (Protocol 29 on mainnet)
- OpenZeppelin Stellar Contracts for access control and pausable
- STELLAR_NETWORK env var, default testnet

## Commands
- cargo test --manifest-path jar/Cargo.toml
- stellar contract build (in jar/)
(not yet runnable — no Cargo project exists yet)

## Rules
- i128 with 7 decimals on-chain. Never floats.
- require_auth on every owner call. No admin function can move user funds.
- Checked arithmetic, no zero or negative amounts, timestamps from the ledger.
- `upgrade` sits behind a 2-of-3 multisig with a 48-hour timelock from mainnet day one.
- withdraw works even when paused.
- A single admin key is for testnet only.

## Workflow
- Write cargo tests first for every function and edge case (zero/negative
  amounts, early withdrawal on a locked jar, non-allowlisted payee, a fake
  token, paused state, TTL extension), then implement.
- External audit required before any mainnet deploy — no exceptions.

## Current phase
Contract written, 45/45 tests passing (39 example-based + 6 property-based),
proven end-to-end on Stellar testnet. UNAUDITED, UNDEPLOYED to mainnet.
Pre-audit hardening attempted: static analysis (Scout, OpenZeppelin
detector SDK) and fuzz testing (cargo-fuzz) both blocked by real tooling/
environment issues documented in README.md — not skipped, just honestly
incomplete. See README.md's "What was not verified" for the precise state
before trusting this with real money.
