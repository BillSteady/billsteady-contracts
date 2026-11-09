# BillSteady Contracts

Soroban smart contracts for [BillSteady](https://github.com/BillSteady/billsteady-web):
pay real Nigerian bills — electricity, airtime, data, school fees —
directly from a Stellar wallet, in XLM or USDC, non-custodially.

## Active: [`router/`](router/README.md) — `BillPaymentRouter`

A thin, non-upgradeable contract: one function moves a payer's own
XLM/USDC straight to a registered settlement address for a specific
biller, in a single atomic transfer. The contract never holds a balance at
any point. **11/11 tests pass, proven end-to-end on testnet** (real
transaction hashes, independently confirmed on Horizon, including a real
negative-case proof that an attacker cannot forge a payer's authorization)
— see [`router/README.md`](router/README.md) for the full detail, the
exact event shape (`billsteady-backend` depends on it), and the admin
trust-boundary decision.

## Archived: [`jar/`](jar/README.md) — the pre-pivot savings jar

BillSteady pivoted from "save over time, then pay from what you saved" to
a pay-now, connect-an-existing-wallet flow (see
[billsteady-landing's CLAUDE.md](https://github.com/BillSteady/billsteady-landing/blob/main/CLAUDE.md)
for the full reasoning). `jar/` is real, tested (45/45), testnet-proven
work — kept as an accurate historical record, not deleted, since the
lock/unlock/early-release machinery it implements may be useful again if
the product ever needs to hold user funds over time.

## Stack

Rust, soroban-sdk 27, Stellar CLI 27.1.0 (re-check against Protocol 29 /
soroban-sdk 28 before any mainnet deploy — built on what was installed,
noted honestly in both contracts' READMEs, not silently assumed current).

## Related repos

- [billsteady-web](https://github.com/BillSteady/billsteady-web) — the wallet-connect + pay-bill PWA
- [billsteady-backend](https://github.com/BillSteady/billsteady-backend) — Supabase backend
- [billsteady-landing](https://github.com/BillSteady/billsteady-landing) — the public landing page

## License

MIT — see [LICENSE](LICENSE).
