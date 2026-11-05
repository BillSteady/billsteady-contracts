# BillSteady Contracts

The Soroban "jar" smart contract for
[BillSteady](https://github.com/bytescape30/billsteady-web): a non-custodial,
user-signed savings jar on Stellar. Users save daily toward a bill; the
contract never gives an admin function the power to move user funds.

## Status: written and tested, UNAUDITED, UNDEPLOYED

This is Phase 3 of the PRD, ahead of the PRD's own sequencing (it was meant
to start after Phase 0-2 prove the naira planner and bill-pay flow with real
users — see [billsteady-web's roadmap](https://github.com/bytescape30/billsteady-web/blob/main/docs/ROADMAP.md)).
It's built now because it needs no external account to start, only the Rust
toolchain, and lives in its own repo precisely so it has an isolated surface
for the external audit the PRD requires before any mainnet deploy — which
has **not** happened. Nothing here has touched testnet or mainnet.

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

- `cargo test` — **39/39 unit tests pass**, covering every function and the
  edge cases called out in the build plan: zero/negative amounts, a locked
  jar before `unlock_time`, a non-allowlisted payee, a disallowed token, the
  paused/not-paused split between `withdraw` and everything else, TTL
  extension, the 24h early-release delay (blocked before, allowed at exactly
  24h), double-release-request rejection, every owner-gated function
  rejecting the wrong caller, and the upgrade gate rejecting a single signer,
  rejecting before the 48h timelock, and rejecting a non-signer outright.
  Test snapshots are committed under `jar/test_snapshots/`.
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

### What was not verified (be honest about this before trusting it)

- **No external audit.** Required before any mainnet deploy per the PRD —
  has not started.
- **Not deployed to testnet.** `cargo test` runs the contract logic against
  `soroban-sdk`'s simulated `Env`, not a real ledger. Deploying to testnet
  and running a create → deposit → pay_out → withdraw script end-to-end
  (Prompt 3.2 in the PRD) hasn't happened yet.
- **No static analysis tooling run** (Scout, OpenZeppelin's detector SDK) —
  recommended by this skill's security guide before requesting an audit.
- **No fuzz or property testing** on `deposit`/`withdraw`/`pay_out` — the
  skill recommends this for every value-moving path.
- **No fork/upgrade rehearsal** against real deployed state.
- **Stellar CLI version mismatch**: built and tested against CLI 27.1.0 /
  soroban-sdk 27.0.6, not the PRD's stated 28.1.0 / Protocol 29. Re-check
  compatibility before any real deploy.

## Related repos

- [billsteady-web](https://github.com/bytescape30/billsteady-web) — the planner PWA
- [billsteady-backend](https://github.com/bytescape30/billsteady-backend) — Supabase backend

## License

MIT — see [LICENSE](LICENSE).
