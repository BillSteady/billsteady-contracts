> **Superseded.** BillSteady pivoted from "save over time, then pay from
> what you saved" to a pay-now, connect-an-existing-wallet flow — see
> [`../router/README.md`](../router/README.md) for the active contract.
> Everything below is kept as an accurate historical record of real,
> tested, testnet-proven work, not deleted — the lock/unlock/early-release
> machinery this contract needed just isn't the right shape for the
> current product. If the product ever needs to hold user funds over time
> again, this is the starting point, not a clean slate.

# BillSteady Jar Contract

The Soroban "jar" smart contract for
[BillSteady](https://github.com/BillSteady/billsteady-web): a non-custodial,
user-signed savings jar on Stellar. Users save daily toward a bill; the
contract never gives an admin function the power to move user funds.

## Status: written, tested, and proven end-to-end on testnet. UNAUDITED, NOT ON MAINNET.

This is Phase 3 of the PRD, ahead of the PRD's own sequencing (it was meant
to start after Phase 0-2 prove the naira planner and bill-pay flow with real
users — see [billsteady-web's roadmap](https://github.com/BillSteady/billsteady-web/blob/main/docs/ROADMAP.md)).
It's built now because it needs no external account to start, only the Rust
toolchain, and lives in its own repo precisely so it has an isolated surface
for the external audit the PRD requires before any mainnet deploy — which
has **not** happened. The contract has been deployed and exercised on
**testnet** (see below) but has never touched mainnet, and testnet proof is
not a substitute for an audit.

### What exists

`jar/src/lib.rs` — a single Soroban contract (`soroban-sdk` 27.0.6, builds
against Stellar CLI 27.1.0; the PRD's doc targeted 28.1.0/soroban-sdk 28,
not yet installed on the machine this was built on — re-verify against the
current SDK before deploying) implementing:

- `create_jar`, `deposit`, `withdraw`, `pay_out`, `request_early_release` /
  `claim_early_release` (24h delay), `close_jar`, `extend_ttl`
- admin: `set_paused`, `set_payees`
- upgrade: `propose_upgrade` / `approve_upgrade` / `cancel_upgrade` /
  `execute_upgrade` — gated by a configurable N-of-M signer set (deployed as
  2-of-3) **and** a 48-hour timelock from when the threshold is met. This is
  implemented in full, not deferred — no single key, including the
  operational admin, can upgrade the contract alone.

### Design decisions worth knowing before you build on this

- **Two separate admin concepts, deliberately.** `OperAdmin` is a single key
  that can `set_paused` and `set_payees` — fast, for incident response. The
  upgrade-signer set is separate (2-of-3 + 48h) because upgrading is the one
  action that can change what the contract does with user funds in the
  future, so it gets the slow, multi-party path. `OperAdmin` is intentionally
  **not** one of the upgrade signers by default in tests — don't conflate them.
- **Pausing never traps funds.** `deposit`, `pay_out`, and `create_jar` are
  blocked while paused (stops new money moving); `withdraw` is explicitly
  exempt, so a paused contract still lets every owner get their own money out.
- **`pay_out` works on a locked jar; `withdraw` doesn't**, until either
  `unlock_time` passes or the 24h early-release delay has been served. That's
  the intended product behavior (pay the bill the jar was for, any time;
  spend it on anything else, only once unlocked).
- **The allowed-token list is set once, at `__constructor`, and has no
  setter.** Adding a new accepted asset later requires an `upgrade`, not an
  admin call. That was a deliberate choice to keep the admin surface small —
  revisit if that turns out to be too rigid operationally.
- **`set_payees` is a trust boundary worth naming explicitly.** `pay_out`
  still requires the jar owner's signature — `OperAdmin` can never move a
  user's funds unilaterally. But `OperAdmin` *does* control which addresses a
  signed `pay_out` is allowed to reach. A compromised `OperAdmin` key can't
  steal funds directly, but could swap in an attacker-controlled "biller"
  address and wait for a legitimate `pay_out` to go to it. Mitigation isn't
  implemented here (e.g. a timelock on payee changes) — flagged for the audit.
- **`close_jar` marks a jar closed rather than deleting its storage entry**,
  trading a small amount of unreclaimed rent for a permanent on-chain record
  future indexers can read.

### What was verified

- `cargo test` — **45/45 tests pass** (39 example-based + 6 property-based),
  covering every function and the edge cases called out in the build plan:
  zero/negative amounts, a locked jar before `unlock_time`, a non-allowlisted
  payee, a disallowed token, the paused/not-paused split between `withdraw`
  and everything else, TTL extension, the 24h early-release delay (blocked
  before, allowed at exactly 24h), double-release-request rejection, every
  owner-gated function rejecting the wrong caller, and the upgrade gate
  rejecting a single signer, rejecting before the 48h timelock, and
  rejecting a non-signer outright. Test snapshots for the example-based
  tests are committed under `jar/test_snapshots/test/`.
- `jar/src/property_test.rs` — 6 `proptest` properties (384 generated cases
  total, 64 per property) locking in the arithmetic invariants the
  example-based tests don't exhaustively cover: a jar's balance after
  deposit/withdraw/pay_out always equals deposits minus withdrawals minus
  payouts and is never negative; a rejected over-withdraw or over-payout
  never mutates the balance; and `deposit`'s `checked_add` genuinely rejects
  overflow (tested by maxing a jar's balance at `i128::MAX`, then confirming
  any further deposit fails with `Error::Overflow` rather than wrapping).
  Property-test snapshots are deliberately **not** committed (they'd churn
  on every run with new random values) — see `.gitignore`.
- `stellar contract build` — compiles clean to
  `target/wasm32v1-none/release/billsteady_jar.wasm`, **14,274 bytes**
  optimized (well under the 128KB network limit), 19 exported functions.
- A manual pass through this skill's security checklist: every privileged
  function's `require_auth` traced to the right address (loaded from
  storage, never a caller-supplied parameter); checked (`checked_add` /
  `checked_sub` / `saturating_add`) arithmetic on every balance and
  timestamp computation; typed `DataKey` enum, no raw storage keys; token and
  payee addresses allowlisted (no arbitrary-contract-call surface); no TTL
  used as a security mechanism.

### Testnet deployment (Prompt 3.2)

Real, independently-verified run on Stellar testnet — not a simulation.
`jar/scripts/testnet-demo.sh` reproduces this: it issues a fresh test asset,
wraps it in a Stellar Asset Contract (SAC), deploys the jar contract, and
runs `create_jar` → `deposit` → `pay_out` → `withdraw` against the real
network, generating fresh throwaway identities each run.

The run recorded below used:
- Jar contract: [`CD62QGT7F4QGECGRS6KN5IOXB2ZPOI7FCPBU2DSUGIFIUC4YC5ZG2V62`](https://stellar.expert/explorer/testnet/contract/CD62QGT7F4QGECGRS6KN5IOXB2ZPOI7FCPBU2DSUGIFIUC4YC5ZG2V62)
- Test token (SAC-wrapped TESTUSDC): [`CAU6NM62R7UHVFR5WQ7Y5I7ODZEEWE7U2DF7JRIKILWIAAYSZQULHJM4`](https://stellar.expert/explorer/testnet/contract/CAU6NM62R7UHVFR5WQ7Y5I7ODZEEWE7U2DF7JRIKILWIAAYSZQULHJM4)

| Step | Tx hash |
|---|---|
| SAC deploy | [`a7adead9bc421cfe098fbfb5a59562dae1149af362014a2d682dbe022fd2d517`](https://stellar.expert/explorer/testnet/tx/a7adead9bc421cfe098fbfb5a59562dae1149af362014a2d682dbe022fd2d517) |
| Jar wasm upload | [`9238e4ae4424932e54f6b44f26e81e8e91458265714ec0b74cc6d12849d231bf`](https://stellar.expert/explorer/testnet/tx/9238e4ae4424932e54f6b44f26e81e8e91458265714ec0b74cc6d12849d231bf) |
| Jar deploy + init | [`d8da182aebdcc34e9203d8c3317862be8b34b2865833ed5fb1337de1c0e21c22`](https://stellar.expert/explorer/testnet/tx/d8da182aebdcc34e9203d8c3317862be8b34b2865833ed5fb1337de1c0e21c22) |
| `create_jar` | [`2029b01d6d893ef7192afa8c8382a798bea4281a5f4fcd322c26d4db40f25e6d`](https://stellar.expert/explorer/testnet/tx/2029b01d6d893ef7192afa8c8382a798bea4281a5f4fcd322c26d4db40f25e6d) |
| `deposit` 20 TESTUSDC | [`d4ff8fc58e8f2752d3176ff03510480146ace5b2029af9ffe0298c83e8befcea`](https://stellar.expert/explorer/testnet/tx/d4ff8fc58e8f2752d3176ff03510480146ace5b2029af9ffe0298c83e8befcea) |
| `pay_out` 5 TESTUSDC to allowlisted payee | [`6841ddc9feb1e96b10a73abc188a5c9031a54e9ef2b9132bb4edaab4bde8ae26`](https://stellar.expert/explorer/testnet/tx/6841ddc9feb1e96b10a73abc188a5c9031a54e9ef2b9132bb4edaab4bde8ae26) |
| `withdraw` 3 TESTUSDC to owner | [`4b34332aed180dd61856700d8456255beb69c9a97017288a04cfcbff7313e0dc`](https://stellar.expert/explorer/testnet/tx/4b34332aed180dd61856700d8456255beb69c9a97017288a04cfcbff7313e0dc) |

Independently confirmed after the run (not just trusting the CLI's "Success"
output): `get_jar` read back `balance: 120000000` (20 deposited − 5 paid out
− 3 withdrawn = 12 remaining, at 7 decimals), and the payee's and owner's
**real classic-asset balances on Horizon** moved by exactly the paid-out and
withdrawn amounts — confirming the SAC transfer moved real underlying
balance, not just emitted an event.

This run hit one real bug in the setup (not the contract): the payee
account had no trustline to TESTUSDC yet, so the first `pay_out` attempt
failed with a genuine `trustline entry is missing` error from the SAC. Fixed
by creating the trustline before retrying — included in the script now.

This exercised the **"jar not locked" withdraw path** (`create_jar` with
`locked=false`), not the 24-hour early-release-delay path
(`request_early_release` → `claim_early_release`) — that path is covered by
`cargo test` with simulated ledger time, since waiting 24 real hours isn't
practical for a demo script. Worth running once against a real elapsed 24h
window before an audit, not just in the simulated test env.

**What this proves:** the contract behaves correctly against the real
Soroban host and network — auth, token transfers, and storage all work as
designed outside the unit-test sandbox. **What it doesn't prove:** that the
contract is safe to hold real money. No audit has happened.

### What was not verified (be honest about this before trusting it)

- **No external audit.** Required before any mainnet deploy per the PRD —
  has not started.
- **The 24h early-release-delay path hasn't been exercised against real
  elapsed time**, only simulated ledger time in `cargo test` (see above).
- **Static analysis: attempted, blocked by real tooling/environment issues,
  not skipped.** Both tools the security skill recommends were tried:
  - **Scout** (`cargo-scout-audit`) failed three distinct ways across two
    pinned versions, none of them our code: v0.3.17's `dylint_driver`
    dependency clones a pinned detector repo and does `git checkout master`
    — that branch doesn't exist upstream (reproduced twice, confirmed via
    `git ls-remote` it's unrelated to network). Pinning to v0.3.11 (the last
    version `docs.rs` could build, per its own build history) needed
    `rustc-dev`+`llvm-tools-preview` on two nightly toolchains (installed)
    to get past that bug, then needed the *exact* `nightly-2025-08-07`
    toolchain its detector crate is pinned to (installed, plus its own
    `rustc-dev`/`llvm-tools-preview`) — and then failed a third time at
    `Building detector-helper` with no further stderr captured. Spent real,
    extended effort on this (confirmed via `cargo scout-audit --version`,
    `--list-detectors` and `--metadata`, which all worked, proving the
    install itself was fine — only the actual lint-compilation pipeline is
    broken here). Did not keep chasing a fourth toolchain change.
  - **OpenZeppelin's Security Detectors SDK** (`soroban-scanner`) has no
    published binary — it must be built from a freshly `git clone`d repo,
    and this sandbox's own code-execution policy correctly refuses to build
    freshly-cloned external code. That's a deliberate, permanent restriction
    in this environment, not a bug to route around.
  - What this repo has instead: a thorough **manual** pass against the
    `stellar-dev:smart-contracts` security checklist (see "What was
    verified" above) — every `require_auth` traced to its real source,
    every arithmetic path confirmed checked, the allowlist/trust-boundary
    analysis that found the `set_payees` issue below. That's real coverage,
    just not tool-automated.
- **Fuzz testing: three real harnesses written
  (`jar/fuzz/fuzz_targets/fuzz_{deposit,withdraw,pay_out}.rs`), none could
  be gotten running.** `cargo-fuzz`'s default (AddressSanitizer) build fails
  to link on this machine with `initializer pointer has no target` — a
  confirmed class of macOS-arm64 + ASan + Rust-nightly linker issue (the
  Soroban SDK's own repo has open reports of the same thing fuzzing on
  arm64 macOS). Tried `-s none` (coverage without ASan): different failure,
  missing libFuzzer runtime symbols. Tried an older pinned nightly with the
  default sanitizer: identical failure to the first attempt, ruling out a
  compiler-version regression specifically. The documented fix (Bitcoin
  Core's own fuzzing docs cite this exact pattern) is building with
  Homebrew LLVM + `lld` instead of Apple's bundled clang — not attempted,
  because installing a new major toolchain component on an unverified "try
  it and see" basis was judged disproportionate to the remaining gain: the
  property tests above already give 384 real randomized executions of the
  exact invariants (balance non-negativity, conservation, overflow
  rejection) that fuzzing would target. The harnesses are committed and
  ready to run the moment this environment's toolchain supports it.
- **No fork/upgrade rehearsal** against real deployed state.
- **Stellar CLI version mismatch**: built and tested against CLI 27.1.0 /
  soroban-sdk 27.0.6, not the PRD's stated 28.1.0 / Protocol 29. Re-check
  compatibility before any real deploy.
