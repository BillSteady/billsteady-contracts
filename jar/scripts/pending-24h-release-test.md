# Pending: real-time 24h early-release test (issue #4)

**Status: request half done, claim half pending real elapsed time.**

This proves `request_early_release` → wait 24h → `claim_early_release`
against real elapsed ledger time on testnet, not `cargo test`'s simulated
clock (see README.md's existing caveat about this).

## What's already done (verified independently against Horizon)

- Dedicated testnet identities (persisted in the local Stellar CLI keystore,
  not ephemeral — `jar-delay-test-admin`, `-issuer`, `-owner`, `-payee`),
  funded by friendbot.
- Test token: SAC `CBUP6E5URFSNNOK6ZYXWDT5H3LHAXW3Y3XYJWNAUXW4LMM6OO7M2XH2S`
  (TESTUSDC, dedicated to this test, not the earlier demo's token).
- Jar contract (dedicated deploy, not the earlier demo's instance):
  `CDI3CC2JGI4I4IMZ4P4GCEYF2A4UCBM6WE3ZHVCESNU2RBMIZ42PX5AO`
- `create_jar` with `locked=true`: tx
  [`c274a71f...8fdeb2`](https://stellar.expert/explorer/testnet/tx/c274a71f5b1994cf05c94b078b66970b459b49a797e7b9952d5df368008fdeb2)
- `deposit` 20 TESTUSDC: tx
  [`979141f2...3fbb36`](https://stellar.expert/explorer/testnet/tx/979141f2a6f792ba27ed4d5dcd27f59ded3d757a3fba317647c254e8ec3fbb36)
- Sanity check: `withdraw` while locked was rejected by the real network
  (`Error(Contract, #8)`) — confirms the jar is genuinely locked, not just
  locked in theory.
- `request_early_release`: tx
  [`1ffd6380...85e572`](https://stellar.expert/explorer/testnet/tx/1ffd6380739bca84d15ca7c2a4085cc438dd5477675ed51025f0d34d6a85e572),
  **confirmed on Horizon** (`successful: true`, ledger 5594579, 2026-11-06T20:22:16Z).
  Contract event: `available_at: 1794082936` (2026-11-07T20:22:16Z — exactly
  24h later, read from the ledger, not client-supplied).

## What's still needed

1. **Before `available_at`** (optional but valuable): attempt
   `claim_early_release` and confirm the real network rejects it, matching
   `cargo test`'s simulated-clock assertion of the same thing.
2. **After `available_at`** (2026-11-07T20:22:16Z or later): run
   `./scripts/claim-early-release-demo.sh`, capture the real tx hash,
   independently confirm it on Horizon, and update README.md's testnet
   section + close GitHub issue #4 with the result.
3. Delete this file once issue #4 is closed — it's a working note, not
   permanent documentation.
