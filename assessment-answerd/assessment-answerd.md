# Solana Assessment — Answers

---

## Section 1 — Solana Fundamentals

Deep dive on Accounts / Programs / PDAs, Anchor vs Native, and treasury risks → [section1.md](section1.md).

## Section 2 — Architecture Review

Deep dive (production readiness, hostile extension list, phased rollout, launch checklist) → [section2.md](section2.md).

### 1. Strengths
- **SPL Token** standard → wallet / DEX / explorer compatibility out-of-box (Phantom, Solflare, Jupiter, Raydium).
- **Anchor** framework → typed accounts, IDL auto-gen, declarative constraints, fewer foot-guns.
- **CPI to `token::transfer`** → battle-tested, atomic, race-condition-safe.

### 2. Limitations
- **CRITICAL: No multisig / governance** — single signer = single point of compromise, can rug unilaterally.
- **HIGH: No Metaplex Token Metadata** — raw mint address visible instead of name / symbol / logo / socials.
- **HIGH: No Token-2022 hardening** — hostile extensions (transfer-hook / fee / permanent-delegate / non-transferable / confidential) not rejected.
- **HIGH: No fee / burn / anti-bot** — zero MEV protection on launch, no deflationary pressure.
- **HIGH: No off-chain indexer** — frontend RPC every tx / holder list; slow + rate-limited.
- **HIGH: No program upgrade path / migration story** — bug ships = users re-migrate funds.
- **MEDIUM: No mobile wallet adapter** — majority of memecoin volume is mobile.
- **MEDIUM: No rate limiting per tx / decimal mismatch / single RPC provider**.

### 3. Security Concerns
- **Authority design**: treasury authority unspecified (likely keypair, not PDA); mint + freeze authorities not revoked → infinite mint / token-freeze hostage.
- **CPI / account validation**: `token::transfer` (unchecked) without decimals; no `has_one = mint`; no `address = known_pubkey`; no event emission → indexers blind.
- **Token-2022 hostile extensions**: full reject list = transfer-hook, transfer-fee, permanent-delegate, non-transferable, confidential-transfer (+ fee), cpi-guard, close-authority, memo-transfer, metadata-pointer, immutable-owner, default-account-state, group-pointer, group-member-pointer.
- **Frontend**: no transaction simulation, no confirmation UX, public RPC.
- **Operational**: upgrade authority not revoked, no timelock on admin ops, no incident response playbook.

### 4. Production Features Needed
- **Token**: Metaplex Token Metadata, Token-2022 hostile-extension refusal at init (full list above).
- **Authority**: Squads multisig m ≥ 3 with hardware-wallet signer + geographically distributed keys; PDA treasury authority (no protocol-controlled keypair); 7-day time-locked `set_authority` handover.
- **Liquidity**: Raydium CPMM pool with **burned** LP (≥ 50 SOL initial to prevent sniper-thin pools); Jupiter aggregator.
- **Frontend**: Mobile Wallet Adapter, `solana-simulate-transaction`, transfer-fee burn-to-treasury deflation.
- **Indexer / ops**: Helius webhooks → PostgreSQL, Grafana, PagerDuty alerts (threshold tuned to % of circulating supply).
- **DevOps**: Jito bundle RPC + priority-fee API, multi-provider fallback (Helius + Triton + QuickNode) with circuit-breaker, CI/CD with anchor-build / test / IDL-publish on tag.
- **Compliance**: ToS + risk disclosure; off-chain OFAC screening at indexer; drop jurisdictional gate (trivially bypassed, worse legal posture).
- **Security**: audit (Neodyme / OtterSec / Trail of Bits) pre-Phase-1; bug bounty (Immunefi) post-launch; multisig upgrade authority with timelock; incident response playbook + on-call rotation.
- **Prerequisites to Phase 1** — supply allocations + cliff vesting (Streamflow), treasury diversification policy, holder-concentration gating metric (top-10 < X%), explicit `pool.paused` flag (NOT freeze authority — already revoked).

### 5. Recommended Next Phase
- **Phase 1 (Hardening, week 1-2)** — Metaplex integration, mint + freeze revoked, PDA authority, Squads m ≥ 3, events, `transfer_checked` + decimals, Token-2022 hostile-ext rejection (full list), `has_one = mint` + `address = vault_pda`, `checked_*` math. **Audit must start BEFORE Phase 1** (3-6 week turnaround + fix-review cycle).
- **Phase 2 (Liquidity, week 3-4)** — Raydium CPMM with burned LP, Jupiter, MWA, transaction simulation, indexer live, Grafana + PagerDuty.
- **Phase 3 (Utility, week 5+)** — Staking (Solana per-user reward_debt + reward_vault idiom, NOT MasterChef — that's EVM), Realms DAO on SPL Gov v3, transfer-fee burn-to-treasury, buy-back-and-burn. **Cross-chain bridge moved to its own phase** (top-3 attack vector by funds lost); NFT scope deferred.

Full launch checklist (27 items) → [section2.md §7](section2.md).

## Section 3 — Code Review

Full audit (Findings table, Patched Implementation, Test matrix) → [section3.md](section3.md).

Full self-contained program + 10-category test stubs → [section3-implementation.md](section3-implementation.md).


---

## Section 4 — Staking System Design

### 1. Required Accounts
- **Pool PDA** `[b"pool", mint]`: reward_rate, acc_reward_per_share (`u128`, 1e18), total_staked, last_update_ts, paused, deposit_enabled, authority, version.
- **Stake PDA** `[b"stake", pool.mint, user]`: amount, unlock_ts, lockup_at_deposit, reward_debt.
- **Vaults**: token accounts owned by Pool PDA, seeds `[b"stake_vault", pool]` / `[b"reward_vault", pool]`, mint-pinned.

### 2. Reward Calculation
MasterChef-style linear emission: `if total_staked > 0 { acc_reward_per_share += reward_rate × Δt × 1e18 / total_staked }`. Pending = `stake.amount × acc / 1e18 − reward_debt`. `u128` fixed-point 1e18, `checked_*` math, `?` propagation.

### 4. Security Considerations
- **First-deposit**: virtual offset in `init_pool` + `deposit_enabled` flag flipped by first `fund_rewards`.
- **Donation**: tracked-vs-actual reconcile; surplus skimmed to treasury (never revert).
- **Reentrancy**: `transfer_checked` + refuse Token-2022 hostile extensions + `is_locked` cleared via Drop impl.
- **Account substitution**: PDAs `#[account(seeds, bump)]`; vault PDAs deterministic-bumped.
- **Flash-loan / lockup**: minimum `lockup_seconds > 0` at init; `now >= stake.unlock_ts` gates BOTH `withdraw_stake` AND `claim_reward`; top-ups preserve `unlock_ts`.
- **MEV / sandwich**: `pending_rate` + `effective_ts` (old rate applies until effective).
- **Multisig**: Squads m ≥ 2 staking admin; `set_authority` 7-day time-locked.
- **State growth**: `close_stake` reaps zero-balance PDAs.
- **Events**: typed `#[event]` on every state transition.

Full deep dive → [section4.md](section4.md).

---

> **Insight:** Staking systems fail most often at math edge cases (first deposit, donation, precision) — which are subtle even when CPI and auth are correct. Accumulator pattern (`accRewardPerShare`) avoids per-user iteration, makes settle `O(1)`. Use it.