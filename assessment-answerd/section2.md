# Section 2 — Architecture Review Deep Dive

> Source: [assessment.txt §2](../assessment.txt#L38). The current project contains SPL Token + Wallet Connection + Token Transfers. Brief asks for 5 sub-items (strengths, limitations, security concerns, production features, next phase), max 1 page. This deep dive expands each with concrete analysis. Companion to [§2](assessment-answerd.md).

---

## 1. Current Strengths

| Component | Strength | Why it matters |
|-----------|----------|----------------|
| SPL Token | Industry-standard token program; battle-tested; integrated with every Solana wallet, explorer, DEX | No reinvention; instant compatibility with Phantom, Solflare, Jupiter, Raydium |
| Anchor framework | Typed accounts, IDL gen, declarative account constraints, error_code macro | Faster dev velocity, fewer foot-guns, auto-generated TS SDK |
| Token Transfers via CPI | Standard `token::transfer` / `token::transfer_checked` pattern | Audited, race-condition-safe, atomic |
| Wallet Connection | Browser wallet adapter (Phantom / Solflare / Backpack) via `@solana/wallet-adapter` | Multi-wallet support without custom integration per wallet |
| Wallet Adapter Standard | dApp stores no private keys; signing happens in user's wallet | Phishing-resistant, user-owned custody |

**Verdict:** the existing setup covers the minimum viable surface (token + wallet + transfer). These primitives are exactly right — they are the same building blocks every production meme coin uses.

---

## 2. Current Limitations

| Gap | Severity | Production impact |
|-----|----------|-------------------|
| **No Metaplex Token Metadata** | HIGH | No logo, name, symbol, description, social links render on explorers / wallets. Only the raw mint address is visible. |
| **No fee / burn / anti-bot mechanism** | HIGH | Zero MEV protection on launch; bots front-run buys; no deflationary supply pressure |
| **No mobile wallet adapter** | MEDIUM | Mobile users (majority of memecoin trading volume on Solana) cannot connect from in-app browser on Phantom Mobile / Solflare Mobile |
| **No off-chain indexer** | HIGH | Frontend must RPC every tx history, holder list, position size. Slow + rate-limited. No historical analytics. |
| **No program upgrade path** | HIGH | If a bug ships, no way to fix without users re-migrating funds. Mechanism: data-account `version: u8` + upgrade authority gated by Squads timelock + `migrate_pool` instruction |
| **No migration story** | HIGH | If mint authority needs to move, no defined flow. Need `migrate_vault` instruction + `set_authority` two-step handover (7-day timelock) |
| **No governance / multisig** | CRITICAL | Single signer key = single point of compromise. Any team member with the key can drain. |
| **No monitoring / alerting** | MEDIUM | No observability for unusual patterns (large transfers, unauthorized mints, vault drain) |
| **No rate limiting per tx** | MEDIUM | Single tx can move unlimited supply; griefer can spam transfers to DoS indexers |
| **No decimal mismatch protection** | MEDIUM | `token::transfer` doesn't enforce decimals; `token::transfer_checked` does. Wrong-decimal use is a recurring loss source. |
| **Single RPC provider** | MEDIUM | One paid endpoint is a worse SPOF than the current setup. Need multi-provider fallback (Helius + Triton + QuickNode) with circuit-breaker |
| **No compliance layer** | LOW | US/EU users interacting with token may trigger securities law; no geo-blocking or OFAC screen |

---

## 3. Security Concerns

### Authority design

| Issue | Risk |
|-------|------|
| Treasury authority unspecified | Could be a keypair (single point of compromise) instead of PDA |
| Mint authority not revoked | Holder of mint authority can inflate supply unbounded → holders diluted → value → 0 |
| Freeze authority not revoked | Holder can freeze any user's tokens → hostage / ransom scenarios |
| No multisig | Single signer can rug unilaterally |

### CPI / account validation

| Issue | Risk |
|-------|------|
| `token::transfer` (unchecked) without decimals | Recipient receives wrong-unit tokens silently |
| No `has_one = mint` on token accounts | Wrong mint passed; tokens move to/from wrong mint |
| No `address = known_pubkey` constraints | Attacker substitutes own token account |
| No event emission | Indexers blind; no audit trail; no proof-of-reserve |

### Token-2022 hostile extensions

If the mint ends up being Token-2022 (or a wrapped version is added later):

| Extension | Risk |
|-----------|------|
| `transfer-hook` | Reentrancy into our program mid-CPI |
| `transfer-fee` | Recipient credits `amount - fee`; accounting silently leaks value |
| `permanent-delegate` | Vault drain independent of our instruction |
| `non-transferable` / `confidential-transfer` / `confidential-transfer-fee` | Silent revert DoS |
| `cpi-guard` | Locks which programs can CPI into our token interactions; misconfiguration can permanently block legitimate flows |
| `close-authority` | Holder can close token accounts, removing holder positions |
| `memo-transfer` | Forces memo on every transfer; UX friction or DoS |
| `metadata-pointer` | Adds external metadata dependency; supply-chain risk |
| `immutable-owner` | Locks owner field; if owner is wrong, no recovery |
| `default-account-state` | Controls whether new ATAs are frozen by default; mint authority can thaw unilaterally |
| `group-pointer` / `group-member-pointer` | Token-2022 group / membership extensions; out of scope for fungible memecoin |

### Frontend

| Issue | Risk |
|-------|------|
| No transaction confirmation UX | Users sign malicious tx from phishing clones |
| No simulation / dry-run | Users don't see what they sign |
| RPC public endpoints | Rate-limited → DoS / poor UX |

### Operational

| Issue | Risk |
|-------|------|
| Program upgrade authority not revoked | Malicious upgrade drains vaults |
| No timelock on admin ops | Same-day malicious action possible |
| No incident response playbook | Hrs-to-days to react to compromise |

---

## 4. Features Needed for Production Launch

### Token layer
- **Metaplex Token Metadata** — name, symbol, URI, logo, social links. Auto-creates metadata PDA via CPI in `create_meme_coin`.
- **Metaplex Master Edition** if NFT-style; not needed for pure fungible.
- **Token-2022 hardening** — refuse hostile extensions at init (TLV walk on mint).
- **Optional Token-2022 features** — confidential transfers (privacy), transfer hooks (custom logic), but ONLY after full security review.

### Authority layer
- **Squads multisig** m ≥ 3 (treasury-controlling, not bare floor) with at least one hardware-wallet signer and geographically distributed keys, for all admin ops: pause, fee updates, mint authority transfer, treasury moves, upgrades.
- **PDA authority** for treasury — no protocol-controlled keypair authority anywhere (signers exist for Squads members + deployer, but on-chain owners are always keys; protocol logic must not depend on a single signer key).
- **Time-locked authority handover** — 7-day wait between `set_authority` call and effect.

### Liquidity layer
- **Raydium CPMM** (Constant Product Market Maker) pool creation with locked LP tokens.
- **LP tokens** burned (single strongest anti-rug) — minimum initial liquidity specified (e.g., ≥ 50 SOL paired + token-side equal value to prevent sniper-thin pools).
- **DEX aggregator** integration via Jupiter API for best-route swaps.

### Frontend layer
- **Mobile Wallet Adapter** (MWA) for in-app wallet on iOS / Android.
- **Transaction simulation** via `solana-simulate-transaction` or Helius Priority Fee API.
- **Confirmation UX** with human-readable instruction breakdown.
- **Transfer-fee burn-to-treasury** mechanism (token-level, not swap-level): configurable % of every transfer is burned, reducing supply.
- **Anti-bot** cooldown per wallet for first N minutes after launch.

### Indexer / observability
- **Off-chain indexer** (Helius webhooks + PostgreSQL / Timescale) for `transfers`, `mints`, `burns`, holder lists.
- **Grafana dashboards** for vault balance, daily volume, top holders, MEV activity.
- **PagerDuty alerts** on: large single-tx outflow, mint authority change, program upgrade.

### Compliance layer
- **Terms of service + risk disclosure** accepted before first transaction (geo-blocking via headers / redirects is trivially bypassed and creates worse legal posture for selective access).
- **Off-chain OFAC screening** at indexer (check addresses against sanctions list).
- **Terms of service + risk disclosure** before first transaction.

### Security
- **Audit** by Neodyme / OtterSec / Trail of Bits before public sale.
- **Bug bounty** public after audit (Immunefi or self-hosted).
- **Multisig-controlled program upgrade authority** with timelock.
- **Incident response playbook** documented, on-call rotation.
- **Disaster recovery**: pause all operations via program-level `pool.paused` flag (NOT freeze authority — already revoked), withdraw liquidity to a cold treasury, communicate via status page.

### DevOps / infra
- **Jito bundle RPC + priority-fee API** (RPC providers do not sell MEV protection; Jito bundle submission protects against JIT-sandwich).
- **CI/CD** for program + indexer (GitHub Actions).
- **Staging → devnet → mainnet** deployment pipeline.
- **Monitoring**: Prometheus metrics, error tracking (Sentry), uptime checks.

---

## 5. Recommended Next Development Phase

### Phase 1 — Hardening (week 1-2)

| Task | Owner | Acceptance |
|------|-------|------------|
| Metaplex Token Metadata integration in `create_meme_coin` | engineer + reviewer | Token shows name/symbol/logo on Solscan + Phantom |
| Mint authority revocation post-init | engineer | `mint.mint_authority == None` after create |
| Freeze authority revocation post-init | engineer | `mint.freeze_authority == None` after create |
| PDA treasury authority (replace any keypair) | engineer + audit | Treasury PDA canonical bump stored, signer seeds use stored bump |
| Squads multisig as `admin` for all CPIs | engineer | All admin instructions gated by `admin_is_authorized` → Squads PDA check |
| `event!` macro on every state transition | engineer | Every instruction emits typed event |
| Switch `token::transfer` → `token::transfer_checked` with mint + decimals | engineer | Decimals validated on every CPI |
| Refuse Token-2022 hostile extensions at vault init | engineer | TLV walk in `init_vault` rejects transfer-hook / transfer-fee / permanent-delegate / non-transferable / confidential / cpi-guard / close-authority / memo-transfer / metadata-pointer / immutable-owner / default-account-state / group-pointer / group-member-pointer |
| `has_one = mint` + `address = vault_pda` on every CPI account set | engineer | Account substitution impossible |
| `checked_*` everywhere on u64 / u128 math | engineer | No silent overflow possible |
| Pre-Phase-1: Audit kickoff (Neodyme / OtterSec) — audits take 3-6 weeks plus fix-review cycle, so this must start BEFORE hardening work begins | external | Audit scope doc + finding-to-fix mapping |
| Post-launch: Bug bounty (Immunefi or self-hosted) with scope docs, asset coverage statement, response SLAs | external | Bounty live within 30 days of launch |

### Phase 2 — Liquidity (week 3-4)

| Task | Owner | Acceptance |
|------|-------|------------|
| Raydium CPMM pool creation instruction | engineer | Pool created from treasury + paired SOL |
| LP tokens burned (single strongest anti-rug; Squads v3+ native `time_lock` config is the alternative if burn is not viable) | engineer | LP cannot be rug-pulled |
| Jupiter aggregator integration in frontend | frontend | Best-route swap shown |
| Mobile wallet adapter support | frontend | Phantom Mobile / Solflare Mobile connect flow |
| Transaction simulation in swap UI | frontend | User sees simulation result before signing |
| Indexer (Helius webhooks → PostgreSQL) | engineer + devops | Real-time transfer / mint / burn feed |
| Grafana dashboard for treasury + holders | devops | Live dashboard accessible |
| PagerDuty alerts on large outflow (>X% of circulating supply in single tx, configurable) | devops | Alert fires on test drain; tune threshold post-launch |

### Phase 3 — Utility (week 5+)

| Task | Owner | Acceptance |
|------|-------|------------|
| Staking program (per-user reward_debt + configurable reward_vault, NOT MasterChef — that's an EVM pattern) | engineer | Stake, claim, withdraw instructions live; boost tiers for lockup |
| Governance (Realms DAO on SPL Gov v3 — production app, not raw SPL Gov SDK) | engineer | Token-weighted voting on proposals |
| Burn-on-transfer mechanism | engineer | % of every transfer reduces supply |
| Buy-back-and-burn from fee revenue | engineer | Auto buy-back from swap fees → burn → deflationary |
| Cross-chain bridge (Wormhole / Mayan) — bridges are top-3 attack vector by funds lost; warrants its own architecture doc + dedicated audit + own phase; NOT in Phase 3 | engineer + audit | Bridge phase declared separately |
| (Removed: NFT scope not committed — defer to explicit feature doc when decided) | — | — |

---

## 6. Tokenomics, Vesting, Treasury Policy (prerequisites to Phase 1)

These items precede all hardening work — token launch with no defined allocations or vesting is a launch blocker.

### Supply schedule + allocations
- Total supply, mint authority lifecycle (revoked post-init), distribution: team %, treasury %, public sale %, liquidity %, advisors %, community airdrops %.
- Cliff + linear vesting for team / advisors / treasury (e.g., 6-month cliff + 24-month linear).
- Public sale via Streamflow or Squads time-lock.

### Vesting implementation
- Streamflow streams for team / advisors (programmatic vesting).
- Squads time-locked treasury (multi-year unlock schedule).
- No team tokens unlocked before public launch.

### Treasury diversification policy
- 7-day timelock authority handover assumes multi-million-SOL treasury.
- Required diversification: ≥ X% in USDC, ≥ Y% in SOL, remainder per policy.
- Documented treasury management policy + on-chain attestation.

### Holder concentration gating metric
- Pre-launch: top-10 holder < Z% of supply.
- Sniper / bundle detection at launch: reject if top-10 > Z% within first hour.
- Insider cluster detection: addresses sharing funding source flagged.

### Emergency-pause model
- Program-level `pool.paused: bool` flag (NOT freeze authority — already revoked).
- Owned by Squads m ≥ 3 multisig.
- Pause blocks deposit / withdraw / fund_rewards; `claim_reward` stays open so users don't get rugged.

### CI/CD scope
- GitHub Actions on tag push: `anchor build`, `anchor test`, IDL regen + publish, program binary publish.
- Pin Rust toolchain + solana-cli / anchor-cli versions.
- Auto-deploy to devnet on `main`; mainnet via manual approval + multisig-controlled upgrade.

---

## 7. Launch Checklist

- [ ] Mint authority revoked post-init
- [ ] Freeze authority revoked post-init
- [ ] PDA treasury authority (no keypair)
- [ ] Squads multisig m ≥ 2 for admin
- [ ] Metaplex Token Metadata live
- [ ] Token-2022 hostile extensions rejected
- [ ] `transfer_checked` everywhere with decimals
- [ ] `has_one = mint` on every CPI
- [ ] Events emitted on every state transition
- [ ] PDA bumps canonicalized and stored
- [ ] `checked_*` math
- [ ] Audit complete + report public
- [ ] Bug bounty live
- [ ] Program upgrade authority = Squads, with timelock
- [ ] Mobile wallet adapter
- [ ] Transaction simulation
- [ ] Indexer live + dashboards
- [ ] PagerDuty alerts configured
- [ ] Paid RPC + MEV protection
- [ ] Terms of service + risk disclosure
- [ ] Off-chain OFAC screening
- [ ] Disaster recovery playbook documented
- [ ] On-call rotation assigned
- [ ] Liquidity pool created + LP locked
- [ ] Jupiter aggregator integrated
- [ ] Staking program (post-launch)
- [ ] Governance program (post-launch)