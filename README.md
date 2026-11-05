# BillSteady Contracts

The Soroban "jar" smart contract for
[BillSteady](https://github.com/bytescape30/billsteady-web): a non-custodial,
user-signed savings jar on Stellar. Users save daily toward a bill; the
contract never gives an admin function the power to move user funds.

## Status: not started

This repo is a placeholder. No contract code exists yet. Per the PRD, this is
Phase 3 of the build and is intentionally sequenced *after* the naira
planner and bill-pay flow prove out (see
[the roadmap](https://github.com/bytescape30/billsteady-web/blob/main/docs/ROADMAP.md)).

Unlike `billsteady-backend`, this one needs no external account to start —
only the Rust + `soroban-sdk` + Stellar CLI toolchain. When work begins, this
repo will contain:

- `jar/` — a Rust/Soroban crate: `create_jar`, `deposit`, `withdraw`,
  `pay_out` (allowlisted payees only), `request_early_release` /
  `claim_early_release` (24h delay), `close_jar`, `extend_ttl`; admin
  `set_paused`, `set_payees`, `upgrade` (behind a 2-of-3 multisig + 48h
  timelock on mainnet)
- `require_auth` on every owner call; checked arithmetic; no admin path can
  move user funds; withdraw works even while paused
- Full test coverage (unit, fuzz, fork) before any testnet deploy, and an
  external audit before any mainnet pilot

## Related repos

- [billsteady-web](https://github.com/bytescape30/billsteady-web) — the planner PWA
- [billsteady-backend](https://github.com/bytescape30/billsteady-backend) — Supabase backend

## License

MIT — see [LICENSE](LICENSE).
