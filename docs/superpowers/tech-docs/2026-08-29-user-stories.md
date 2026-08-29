# User Stories — Meme Coin Build

> Derived from `assessment.txt` + 7 superpowers plans (`docs/superpowers/plans/`) + reference answers (`assessment-answerd/`).
> Format: As a [role], I want [feature], so that [benefit]. Each story carries acceptance criteria, plan source, priority.

## Convention

- **P0** = must ship before any mainnet/airdrop launch (security-critical)
- **P1** = required for MVP launch
- **P2** = post-launch enhancements
- **Role** = single source-of-truth persona (admin, user, integrator, auditor, bot)

`★ Insight ─────────────────────────────────────`
Stories split into 4 role clusters: **Admin** (governs supply + vault), **End User** (claims / buys / stakes), **Integrator** (off-chain bots, wallets, indexers), **Auditor** (verifies invariants). Each story references the plan task that ships it + the test that proves it. Stories are dependency-ordered: hostile-extension guard (US-A2) blocks every module story because `common::assert_no_hostile_extensions` is referenced in all 6 module handlers.
`─────────────────────────────────────────────────`

---

## 1. Admin / operator stories

### US-A1 — Initialize the meme-coin mint
- **As a** token deployer,
- **I want** to call `initialize_token(name, symbol, uri, total_supply)` once,
- **So that** the supply is fixed, mint authority is a PDA, and freeze authority is revoked.
- **Priority**: P0
- **Plan source**: token plan Task 3 + 6 (state + handlers + Metaplex)
- **Acceptance**:
  - Mint created with `decimals = 9`, `total_supply` minted to treasury ATA.
  - `spl-token mint-info <mint>` shows `mint-authority: None`, `freeze-authority: None`.
  - Metaplex metadata PDA visible with `update_authority: None`.
  - `TokenInitialized` event emitted with correct fields.
- **Test**: `cargo test -p meme-coin-tests initialize_token_ix_routes` + manual `spl-token mint-info`.

### US-A2 — Reject Token-2022 hostile mints
- **As a** protocol guard,
- **I want** every mint-touching instruction to reject mints with hostile Token-2022 extensions,
- **So that** users cannot bypass fee/transfer-hook logic via tainted mints.
- **Priority**: P0
- **Plan source**: workspace plan Tasks 10-11 (common.rs guard) + every module plan Task 3 (guard call)
- **Acceptance**:
  - Mint with `TransferFeeConfig` extension → reverts with `HostileMintExtension`.
  - Empty mint data → passes.
  - All 14 `HOSTILE_EXTENSIONS` covered (TransferHook, TransferFeeConfig, PermanentDelegate, NonTransferable, ConfidentialTransferMint, ConfidentialTransferFeeConfig, CpiGuard, CloseAuthority, MemoTransfer, MetadataPointer, ImmutableOwner, DefaultAccountState, GroupPointer, GroupMemberPointer).
- **Test**: `cargo test -p meme-coin-tests initialize_token_rejects_hostile_transfer_fee_mint`.

### US-A3 — Initialize an airdrop
- **As a** community manager,
- **I want** to publish a Merkle-rooted allowlist + time window + total budget,
- **So that** whitelisted users can claim exactly their share, no more.
- **Priority**: P1
- **Plan source**: airdrop plan Task 3 (`initialize_airdrop`)
- **Acceptance**:
  - `AirdropState` PDA created with `merkle_root`, `start_ts`, `end_ts`, `total_tokens`.
  - Vault (PDA-owned token account) created and visible at `airdrop.vault`.
  - `AirdropInitialized` event emitted.
- **Test**: covered by `airdrop_end_to_end_initialize_fund_claim_double_claim`.

### US-A4 — Fund the airdrop vault
- **As a** treasury operator,
- **I want** to transfer tokens from my wallet into the airdrop vault,
- **So that** claimers can be paid out as they come.
- **Priority**: P1
- **Plan source**: airdrop plan Task 3 (`fund_airdrop_vault`)
- **Acceptance**:
  - SPL transfer from `funder_token_account` → `vault`.
  - Vault PDA owned, `has_one = mint` enforced.
  - `AirdropVaultFunded` event emitted with amount.
- **Test**: covered by `airdrop_end_to_end_initialize_fund_claim_double_claim`.

### US-A5 — Initialize presale with tier pricing
- **As a** launch operator,
- **I want** to publish a tiered presale with soft cap + hard cap + time window,
- **So that** buyers get transparent price discovery and refunds if soft cap fails.
- **Priority**: P1
- **Plan source**: presale plan Task 1 + 3 (`initialize_presale`)
- **Acceptance**:
  - `PresaleState` PDA created with `start_ts < end_ts`, `0 < soft_cap <= hard_cap`, non-empty `tiers`.
  - Tiers stored as `Vec<TierPrice>`, no floats.
  - `PresaleInitialized` event emitted.
- **Test**: covered by `presale_buy_records_state_correctly`.

### US-A6 — Finalize presale (success or refund path)
- **As a** launch operator,
- **I want** to call `finalize_presale()` after the window ends,
- **So that** SOL either flows to treasury (soft cap hit) or refunds become available.
- **Priority**: P1
- **Plan source**: presale plan Task 3 (`finalize_presale`)
- **Acceptance**:
  - Reverts before `end_ts` with `TooEarly`.
  - If `total_lamports >= soft_cap`: SOL moved vault → treasury, `reached_soft_cap = true`, `PresaleFinalized { reached_soft_cap: true }`.
  - Else: `reached_soft_cap = false`, `claim_refund` enabled.
- **Test**: covered by `presale_buy_records_state_correctly` (success path); refund path untested in plans.

### US-A7 — Initialize a vesting schedule for a beneficiary
- **As a** multisig authority,
- **I want** to upload a stored `Vec<ReleasePoint>` schedule per beneficiary,
- **So that** the beneficiary can release tokens according to a verifiable, on-chain curve.
- **Priority**: P1
- **Plan source**: vesting plan Task 1 + 2 (`initialize_vesting`)
- **Acceptance**:
  - `schedule` length <= `MAX_RELEASE_POINTS = 256`.
  - `schedule` amounts sum to `total_amount` (else revert `InvalidArgument`).
  - `cliff_ts < end_ts` enforced.
  - Vault created, owned by vault_authority PDA.
  - `VestingInitialized` event emitted.
- **Test**: covered by `vesting_release_before_cliff_reverts`.

### US-A8 — Revoke a vesting schedule
- **As a** multisig authority,
- **I want** to halt future releases for a given beneficiary,
- **So that** unvested tokens stay in the vault (already-released stays).
- **Priority**: P1
- **Plan source**: vesting plan Task 2 (`revoke_vesting`)
- **Acceptance**:
  - Only callable by `authority` signer (`has_one = authority`).
  - Sets `revoked = true`. Future `release_vested` reverts with `VestingRevoked`.
  - `VestingRevoked` event emitted.
- **Test**: not covered in plans; add `vesting_revoke_halts_future_release`.

### US-A9 — Initialize a Raydium CPMM pool with locked LP
- **As a** launch operator,
- **I want** to create a token/SOL Raydium pool and immediately burn the LP to a known address,
- **So that** liquidity is provably locked from minute one.
- **Priority**: P1
- **Plan source**: liquidity plan Tasks 1-3 (`initialize_pool`)
- **Acceptance**:
  - Raydium CPMM pool created with `token_amount` + `sol_amount` from PDA vaults.
  - LP tokens transferred to `lp_burn_destination` (post-CPI balance read).
  - `PoolInitialized` event emitted with `lp_burned > 0`.
  - Post-deploy verification on Raydium UI confirms LP locked.
- **Test**: `liquidity_module_reachable_from_tests` (fixture only — full test gated on Raydium fork fixture).

### US-A10 — Initialize a staking pool
- **As a** protocol admin,
- **I want** to launch a stake/reward pool with emission rate + lockup,
- **So that** token holders earn rewards over time.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`init_staking_pool`)
- **Acceptance**:
  - `lockup_seconds > 0` (else revert).
  - Both `stake_mint` + `reward_mint` pass hostile-extension guard.
  - `acc_reward_per_share = ONE` at init (first-staker protection).
  - `deposit_enabled = false` until first `fund_rewards`.
  - `StakingPoolInitialized` event emitted.
- **Test**: covered by `staking_ix_handlers_exported` (symbol-only); full integration test not in plans.

### US-A11 — Fund a staking reward vault
- **As a** reward distributor,
- **I want** to deposit reward tokens into the pool's vault,
- **So that** emission has backing and donations get skimmed back.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`fund_rewards`)
- **Acceptance**:
  - SPL transfer funder → vault.
  - `settle_pending_rewards` called first (acc update).
  - Donation guard: if `vault.amount > expected`, skim surplus back to funder.
  - Sets `deposit_enabled = true` on first fund.
  - `RewardsFunded` event emitted.
- **Test**: not covered in plans; add `staking_fund_rewards_skims_donations`.

### US-A12 — Pause + emergency withdraw from staking
- **As a** multisig authority,
- **I want** to pause deposits/withdraws and let users emergency-exit (forfeit rewards),
- **So that** users can recover principal if the pool is compromised.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`emergency_withdraw`)
- **Acceptance**:
  - `emergency_withdraw` only callable when `pool.paused = true`.
  - Returns full `stake_entry.amount` to user.
  - Zeroes `stake_entry.amount` + `reward_debt`.
  - `EmergencyWithdrawn` event emitted.
- **Test**: not covered in plans; add `staking_emergency_withdraw_returns_principal`.

---

## 2. End user stories

### US-U1 — Claim an airdrop allocation
- **As a** whitelisted user,
- **I want** to call `claim_airdrop(amount, proof)` with my Merkle proof,
- **So that** I receive my tokens and cannot double-claim.
- **Priority**: P0
- **Plan source**: airdrop plan Task 3 (`claim_airdrop`)
- **Acceptance**:
  - `now in [start_ts, end_ts)` else `TooEarly` / `TooLate`.
  - Merkle proof verifies → else `InvalidProof`.
  - Per-user `Claim` PDA `init` constraint enforces one-claim-per-user.
  - SPL transfer via PDA-signed vault authority.
  - `claimed_count` increments.
  - `AirdropClaimed` event emitted.
- **Test**: `airdrop_end_to_end_initialize_fund_claim_double_claim` (asserts double-claim fails).

### US-U2 — Buy presale tokens
- **As a** retail buyer,
- **I want** to call `buy_tokens(amount, expected_total_lamports)`,
- **So that** I receive tokens at the tier price with slippage protection.
- **Priority**: P1
- **Plan source**: presale plan Task 3 (`buy_tokens`)
- **Acceptance**:
  - `now in [start_ts, end_ts)`.
  - `total_cost <= expected_total_lamports` else `SlippageExceeded`.
  - `amount <= max_buyable_in_tier` else `HardCapExceeded`.
  - `system_program::transfer` buyer → SOL vault.
  - PDA-signed `mint_to` to buyer's ATA.
  - `Contribution` PDA accumulates per-buyer totals.
  - `PresaleBought` event emitted.
- **Test**: `presale_buy_records_state_correctly`.

### US-U3 — Claim a presale refund
- **As a** presale buyer after soft-cap fail,
- **I want** to call `claim_refund()`,
- **So that** I recover the SOL I contributed.
- **Priority**: P1
- **Plan source**: presale plan Task 3 (`claim_refund`)
- **Acceptance**:
  - Only callable after `finalize` + `!reached_soft_cap`.
  - One refund per buyer (`refunded` flag).
  - Returns exact `lamports_paid` from contribution PDA.
  - `PresaleRefunded` event emitted.
- **Test**: not covered in plans; add `presale_claim_refund_returns_lamports`.

### US-U4 — Release vested tokens
- **As a** vesting beneficiary,
- **I want** to call `release_vested()` after the cliff,
- **So that** I receive my unlocked tokens.
- **Priority**: P1
- **Plan source**: vesting plan Task 2 (`release_vested`)
- **Acceptance**:
  - `now >= cliff_ts` else `CliffNotReached`.
  - `!revoked` else `VestingRevoked`.
  - `amount_to_release = releasable_now - total_released`.
  - PDA-signed SPL transfer vault → beneficiary ATA.
  - `VestingReleased` event emitted.
- **Test**: `vesting_release_before_cliff_reverts`.

### US-U5 — Stake tokens
- **As a** token holder,
- **I want** to call `stake(amount)`,
- **So that** I start earning rewards subject to lockup.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`stake`)
- **Acceptance**:
  - `deposit_enabled && !paused`.
  - SPL transfer user → stake vault.
  - First deposit sets `unlock_ts = now + lockup_seconds`; top-ups preserve.
  - `acc_reward_per_share` settled first.
  - `reward_debt = amount * acc_reward_per_share / ONE`.
  - `Staked` event emitted with `lockup_at_deposit`.
- **Test**: not covered in plans; add `staking_stake_locks_until_unlock`.

### US-U6 — Withdraw stake
- **As a** staker after lockup,
- **I want** to call `withdraw_stake(amount)`,
- **So that** I recover principal.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`withdraw_stake`)
- **Acceptance**:
  - `now >= unlock_ts` else `LockupActive`.
  - `amount <= stake_entry.amount` else `InsufficientUnlocked`.
  - PDA-signed SPL transfer vault → user.
  - `acc_reward_per_share` settled.
  - `total_staked` decremented.
  - `Withdrawn` event emitted.
- **Test**: not covered in plans; add `staking_withdraw_after_lockup_returns_principal`.

### US-U7 — Claim staking rewards
- **As a** staker after lockup,
- **I want** to call `claim_reward()`,
- **So that** I receive accumulated reward tokens.
- **Priority**: P1
- **Plan source**: staking plan Task 3 (`claim_reward`)
- **Acceptance**:
  - `now >= unlock_ts` else `LockupActive`.
  - Settles rewards, computes `pending_reward`.
  - PDA-signed SPL transfer reward vault → user.
  - `reward_debt += amount * ONE`.
  - `RewardClaimed` event emitted.
- **Test**: not covered in plans; add `staking_claim_reward_returns_accrued`.

---

## 3. Integrator / off-chain bot stories

### US-I1 — Subscribe to program events via Helius webhook
- **As a** UI/indexer developer,
- **I want** a Helius `ENHANCED_TRANSACTIONS` webhook filtered by the program id,
- **So that** I get every event log decoded via the published IDL.
- **Priority**: P1
- **Plan source**: workspace plan Task 14 (`docs/indexer-webhooks.md`)
- **Acceptance**:
  - Webhook URL populated in deployment config (not committed).
  - All 19 event types present in IDL.
  - Index keys documented (mint, airdrop, presale, pool, user × pool, etc.).
- **Test**: manual `anchor deploy` + Helius dashboard subscribe.

### US-I2 — Decode IDL for instruction building
- **As a** wallet frontend,
- **I want** `target/idl/meme_coin.json` + `target/types/meme_coin.ts`,
- **So that** I can construct + serialize every instruction client-side.
- **Priority**: P1
- **Plan source**: workspace plan Task 4 (`idl-build` feature) + every module plan
- **Acceptance**:
  - `anchor build` emits IDL with all 18+ ix variants.
  - `meme_coin::instruction::*::data()` callable in tests (proves IDL is consistent).
- **Test**: every integration test uses `meme_coin::instruction::X::data()`.

### US-I3 — Detect hostile mint via indexer alert
- **As a** security monitor,
- **I want** an alert when `HostileMintExtension` error fires,
- **So that** I can flag suspicious mint attempts.
- **Priority**: P2
- **Plan source**: workspace plan Task 14 (alert thresholds)
- **Acceptance**:
  - Alert path documented in `docs/indexer-webhooks.md`.
  - PagerDuty integration wired.
- **Test**: manual inject HostileMintExtension → webhook fires.

### US-I4 — Alert on treasury drain
- **As a** ops engineer,
- **I want** an alert when treasury token balance drops > 5% in 1 minute,
- **So that** I catch drains before they're catastrophic.
- **Priority**: P2
- **Plan source**: workspace plan Task 14 (account webhook + alert thresholds)
- **Acceptance**:
  - `ACCOUNT` webhook subscribed to treasury ATA + reward vaults.
  - Grafana panel surfaces trend.
- **Test**: manual drain simulation.

### US-I5 — Verify pool initialization on Raydium UI
- **As a** launch auditor,
- **I want** a manual check that LP tokens are in the burn address,
- **So that** I can confirm liquidity is locked before announcing.
- **Priority**: P1
- **Plan source**: liquidity plan acceptance criteria
- **Acceptance**:
  - Post-init, Raydium UI shows LP supply 0 (burned).
- **Test**: manual post-deploy check.

---

## 4. Auditor / security stories

### US-S1 — No f64 in money math
- **As a** security auditor,
- **I want** all price/amount math to use integer types only (u64 / u128),
- **So that** no rounding errors lose funds.
- **Priority**: P0
- **Plan source**: workspace plan global constraints + presale plan (tier pricing)
- **Acceptance**:
  - `cargo clippy` clean (catches `f64` / `f32` casts).
  - `tiers.rs` uses `Vec<TierPrice { cap_sold: u64, price_lamports_per_token: u64 }`.
  - `settle_pending_rewards` uses u128 with ONE = 1e18.
- **Test**: `cargo clippy --workspace --all-targets -- -D warnings` clean.

### US-S2 — All math uses checked_*
- **As a** security auditor,
- **I want** every arithmetic op wrapped in `checked_add / sub / mul / div`,
- **So that** overflow reverts cleanly instead of panicking.
- **Priority**: P0
- **Plan source**: workspace plan global constraints + every module plan
- **Acceptance**:
  - No `+`, `-`, `*`, `/` on `u64` / `u128` outside checked variants.
  - `overflow-checks = true` in workspace release profile.
- **Test**: `cargo clippy --workspace --all-targets -- -D warnings` + fuzz trident.

### US-S3 — Single ErrorCode enum (no parallel enums)
- **As a** security auditor,
- **I want** one `ErrorCode` enum per program (per lesson L2),
- **So that** clients can decode errors uniformly.
- **Priority**: P0
- **Plan source**: workspace plan Task 7 + lesson L2
- **Acceptance**:
  - `programs/meme-coin/src/errors.rs` = single enum.
  - No `pub enum XxxError` anywhere.
- **Test**: `grep -r "pub enum.*Error" programs/meme-coin/src/` returns one match.

### US-S4 — All event fields that indexers filter on are `#[index]`-ed
- **As a** security auditor,
- **I want** every Pubkey / mint / authority / role field on `#[event]` structs tagged `#[index]`,
- **So that** indexers can build query plans.
- **Priority**: P0
- **Plan source**: workspace plan Task 8 + lesson L3
- **Acceptance**:
  - 19 events × N indexed fields each. Grep confirms.
- **Test**: `grep -c "#\[index\]" programs/meme-coin/src/events.rs` >= 30.

### US-S5 — PDA bumps stored on-chain
- **As a** security auditor,
- **I want** every state account to store its PDA bump (not re-derived per ix),
- **So that** the canonical bump is reused + CU is saved.
- **Priority**: P0
- **Plan source**: every module plan (state.rs files)
- **Acceptance**:
  - Every `#[account(seeds = [...])]` state has a `_bump: u8` field.
  - Handler uses `bump = ctx.bumps.X` constraint.
- **Test**: code review of every `state.rs`.

### US-S6 — Reentrancy-safe state updates
- **As a** security auditor,
- **I want** every state mutation to happen before external CPI,
- **So that** a re-entrant call sees updated state.
- **Priority**: P0
- **Plan source**: every handler
- **Acceptance**:
  - Airdrop `claim`: `claimed_count` updated after CPI (post-token-transfer).
  - Presale `buy`: state updated after CPI.
  - Staking `stake/withdraw/claim`: `settle_pending_rewards` before CPI.
- **Test**: trident fuzz + manual reentrancy probe.

### US-S7 — No client-supplied authority
- **As a** security auditor,
- **I want** every authority field to be derived from PDA seeds, never from a wallet keypair,
- **So that** mint/freeze/upgrade control cannot be seized.
- **Priority**: P0
- **Plan source**: every module plan (mint authority = PDA, not wallet)
- **Acceptance**:
  - Token plan: mint authority = `MINT_AUTHORITY_SEED` PDA, not user.
  - Airdrop: vault authority = `VAULT_SEED` PDA.
  - Presale: mint authority = `presale_mint_authority` PDA.
  - All have `mint_authority: None` post-revoke.
- **Test**: `spl-token mint-info` post-init shows PDA + None.

### US-S8 — Upgrade authority set to multisig post-launch
- **As a** security auditor,
- **I want** the program's upgrade authority to be a Squads multisig (not a single keypair),
- **So that** no single party can upgrade the program unilaterally.
- **Priority**: P1
- **Plan source**: not in plans — must add to deployment plan
- **Acceptance**:
  - `solana program show <id>` shows upgrade authority = Squads vault.
  - Squads threshold >= 2-of-N.
- **Test**: post-deploy verification.

---

## 5. DevOps / CI stories

### US-D1 — CI builds + lints + tests on every PR
- **As a** maintainer,
- **I want** GitHub Actions to run `cargo fmt --check`, `cargo clippy -D warnings`, `anchor build`, `anchor test`,
- **So that** no PR lands broken.
- **Priority**: P0
- **Plan source**: workspace plan Task 13
- **Acceptance**:
  - CI green on `assessment` branch.
  - Pin toolchain via `dtolnay/rust-toolchain@1.79.0`.
  - Pin Solana CLI `v1.18.26`.
- **Test**: PR triggers CI, all jobs green.

### US-D2 — Toolchain pinned across workspace + manifests
- **As a** maintainer,
- **I want** Rust + Solana CLI + Anchor CLI versions committed,
- **So that** builds are reproducible.
- **Priority**: P0
- **Plan source**: workspace plan Tasks 1-2 + lesson L4
- **Acceptance**:
  - `rust-toolchain.toml` pins `1.79.0`.
  - `package.json` engines pin `node >=20` + `anchor-cli 0.30.1`.
  - `Cargo.toml` workspace metadata pins `anchor-cli 0.30.1` + `solana 1.18.26`.
- **Test**: fresh clone + `cargo check` succeeds without prompting.

### US-D3 — Indexer webhook schema documented
- **As an** ops engineer,
- **I want** `docs/indexer-webhooks.md` referencing every event type + alert threshold,
- **So that** ops can subscribe before launch.
- **Priority**: P1
- **Plan source**: workspace plan Task 14
- **Acceptance**:
  - Doc references all 19 events.
  - Alert thresholds + PagerDuty routing documented.
- **Test**: doc exists + grep matches every event name.

---

## 6. Coverage matrix (stories → plans → tests)

| Story | Plan Task(s) | Test |
|---|---|---|
| US-A1 | token T3, T6 | `initialize_token_ix_routes` |
| US-A2 | workspace T10-11 + every module T3 | `initialize_token_rejects_hostile_transfer_fee_mint` |
| US-A3, US-A4 | airdrop T3 | `airdrop_end_to_end_initialize_fund_claim_double_claim` |
| US-A5 | presale T1, T3 | `presale_buy_records_state_correctly` |
| US-A6 | presale T3 | (none — add) |
| US-A7 | vesting T1, T2 | `vesting_release_before_cliff_reverts` |
| US-A8 | vesting T2 | (none — add) |
| US-A9 | liquidity T1-3 | `liquidity_module_reachable_from_tests` |
| US-A10, US-A11, US-A12 | staking T3 | `staking_ix_handlers_exported` (symbol only) |
| US-U1 | airdrop T3 | `airdrop_end_to_end_initialize_fund_claim_double_claim` |
| US-U2 | presale T3 | `presale_buy_records_state_correctly` |
| US-U3 | presale T3 | (none — add) |
| US-U4 | vesting T2 | `vesting_release_before_cliff_reverts` |
| US-U5, US-U6, US-U7 | staking T3 | (none — add) |
| US-I1, US-I3, US-I4 | workspace T14 | (manual) |
| US-I2 | workspace T4 | every integration test |
| US-I5 | liquidity plan acceptance | (manual) |
| US-S1, US-S2 | workspace constraints | `cargo clippy -- -D warnings` |
| US-S3 | workspace T7 | `error_codes_are_exported` |
| US-S4 | workspace T8 | `event_types_are_exported` |
| US-S5 | every module state.rs | (code review) |
| US-S6 | every handler | trident fuzz |
| US-S7 | every module plan | `spl-token mint-info` |
| US-S8 | (deployment plan) | post-deploy |
| US-D1 | workspace T13 | CI green |
| US-D2 | workspace T1-2, L4 lesson | fresh clone |
| US-D3 | workspace T14 | doc grep |

`★ Insight ─────────────────────────────────────`
**Test coverage gap**: presale finalize success path, vesting revoke, refund path, all staking integration tests (4 stories) have zero integration tests in plans. These are the highest-value integration tests to add next. Staking is the biggest gap — 5 stories ship with only symbol-presence tests.
`─────────────────────────────────────────────────`

---

## 7. Priority rollup

### P0 (must ship before launch)

US-A1, US-A2, US-U1, US-S1, US-S2, US-S3, US-S4, US-S5, US-S6, US-S7, US-D1, US-D2

= 12 stories. All block mainnet/airdrop.

### P1 (required for MVP launch)

US-A3, US-A4, US-A5, US-A6, US-A7, US-A8, US-A9, US-A10, US-A11, US-A12, US-U2, US-U3, US-U4, US-U5, US-U6, US-U7, US-I1, US-I2, US-I5, US-S8, US-D3

= 21 stories. All required for production-grade launch.

### P2 (post-launch enhancements)

US-I3, US-I4

= 2 stories. Indexer-side monitoring polish.

---

## 8. Recommended execution order (story-driven)

1. **US-A2** (workspace T10-11) — unblocks every other story. ~1-2 hours.
2. **US-A1** + **US-S7** + **US-S5** (token T1-6) — proves mint authority pattern. ~1 day.
3. **US-A3, US-A4, US-U1** + **US-S3, US-S4** (airdrop T1-5) — proves Merkle pattern + Claim PDA. ~1 day.
4. **US-A5, US-A6, US-U2, US-U3** + **US-S1** (presale T1-5) — proves tier pricing + system CPI + slippage. ~1-2 days.
5. **US-A7, US-A8, US-U4** (vesting T1-4) — proves stored schedule + cliff + revoke. ~0.5 day.
6. **US-A9** (liquidity T1-5) — gated on Raydium SDK pin. ~1-2 days.
7. **US-A10, US-A11, US-A12, US-U5, US-U6, US-U7** (staking T1-5) — gated on math correctness test (cheap first). ~2-3 days.
8. **US-I1, US-I3, US-I4, US-D3** (workspace T14 + indexer doc) — parallel during build. ~0.5 day.
9. **US-D1** (workspace T13 CI) — must be green before any merge. ~0.5 day.
10. **US-I5, US-S8** — deployment + multisig + manual Raydium verify. ~0.5 day.

**Total**: ~7-10 working days for full MVP.
