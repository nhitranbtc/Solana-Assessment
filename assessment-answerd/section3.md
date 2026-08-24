# Section 3 Audit — `transfer_rewards`

> Source: [assessment.txt §3](../assessment.txt#L65). Merged from `ecc:rust-reviewer` + `compass:security-auditor` + actual review of `airdrop.rs`, `dev_fund.rs`, `lib.rs`, `liquidity.rs`, `presale.rs`, `token.rs` (all present at repo root). Cross-cutting pattern issues flagged under §Cross-Cutting Patterns.

## Snippet

```rust
pub fn transfer_rewards(ctx: Context<TransferRewards>, amount: u64) -> Result<()> {
    token::transfer(
        ctx.accounts.transfer_context(),
        amount,
    )?;

    Ok(())
}
```

---

## Findings — Merged (severity DESC)

| Severity | Line | Finding | Fix |
|----------|------|---------|-----|
| CRITICAL | L1   | `amount` unvalidated — zero wastes compute, oversized causes opaque revert / DoS | `require!(amount > 0 && amount <= source.amount, ErrorCode::InvalidAmount)` |
| CRITICAL | STRUCT | Account set unconstrained — attacker passes own ATA as `recipient_token` | typed `source`/`recipient_token`/`mint` + `has_one = mint` + `address = anchor_spl::token::ID` on `token_program` |
| CRITICAL | STRUCT | CPI authority unverifiable — `transfer_context()` helper hides signer seeds | inline `CpiContext::new_with_signer(...)` with explicit `signer_seeds` |
| HIGH | STRUCT | No caller signer — anyone invokes `transfer_rewards` | `admin: Signer` + `#[access_control(admin_is_authorized(...))]` wired to Squads multisig |
| HIGH | STRUCT | PDA bump canonicalization not enforced | `#[account(seeds = [b"vault", mint.key()], bump)]` + reuse stored bump in CPI signer seeds |
| HIGH | L2-L5 | `transfer_context()` helper opaque — silent transfer-from-PDA failure or wrong authority signed | drop helper, build `CpiContext` inline, unit-test signer seeds byte-for-byte |
| HIGH | STRUCT | Mint mismatch across source / recipient / expected | `mint.key() == EXPECTED_MEME_MINT` constraint + `has_one` on both token accounts |
| HIGH | STRUCT | Token-2022 transfer-hook extension → reentrancy or DoS | reject hook-bearing mints at vault init |
| HIGH | STRUCT | Token-2022 transfer-fee extension → recipient credits `amount - fee`, value leak to fee recipient | `transfer_checked` OR query `transfer_fee_config` + pre-debit, OR reject fee-bearing mints |
| HIGH | STRUCT | Token-2022 permanent-delegate extension → vault drain independent of this instruction | reject mint at init |
| HIGH | STRUCT | Token-2022 non-transferable / confidential / transfer-gated → silent revert DoS | allowlist compatible mints only |
| MEDIUM | META | No event emission — indexers / accounting / audit blind | `emit!(RewardsTransferred { recipient, mint, vault, amount, ts })` |
| MEDIUM | META | Vault authority design — single keypair = single failure | PDA authority; multisig + timelock if external control needed |
| MEDIUM | STRUCT | Multisig gap if authority is SPL multisig | validate multisig `m >= 2` and signer set against policy |
| MEDIUM | META | Reentrancy via Token-2022 hook | checks-effects-interactions: update state before CPI, or refuse hook mints entirely |
| MEDIUM | META | Per-recipient / per-epoch cap missing | PDA epoch counter, reserve rent-exempt minimum |
| MEDIUM | META | MEV / sandwich — recipient ATA mempool-visible | private mempool submission, batched payouts, claim-then-vest cooldown |
| MEDIUM | L2 | `token::transfer` no decimals check — recipient may receive units they did not expect | `transfer_checked` with mint + decimals in CPI |
| MEDIUM | L1 | Arithmetic unchecked downstream — `amount` may exceed u64 math elsewhere | `checked_add` / `checked_sub` on all aggregations |
| MEDIUM | L5 | Bare `?` leaks SPL error codes as stable oracle | `.map_err(\|_\| error!(ErrorCode::TokenTransferFailed))?` |
| MEDIUM | META | Post-transfer invariant missing — Token-2022 fee drift undetected | `source.reload()?` + `destination.reload()?`, assert `destination.amount - prev >= amount` |
| LOW | META | No rate-limit / per-epoch cap | `epoch.remaining >= amount` constraint on a per-epoch PDA |
| CRITICAL | CROSS-CUTTING | All modules (`airdrop.rs:73`, `dev_fund.rs:51`, `presale.rs:40`, `token.rs:23`) use external `authority: Signer<'info>` as CPI authority instead of a program-derived PDA — whoever holds that keypair can drain the relevant token account | replace with PDA authority: `seeds = [b"vault", mint.key().as_ref()]`, `bump`, and sign via `CpiContext::new_with_signer` |
| CRITICAL | CROSS-CUTTING | `dev_fund.rs:54-58` and `presale.rs:43-47` call `token::transfer(program, accounts, amount)` with the 3-argument form — Anchor 0.30 expects `CpiContext::new(program, accounts)` then `token::transfer(cpi_ctx, amount)`; the call as-written does not match the current Anchor API and will fail | use `CpiContext::new(token_program, Transfer{from, to, authority})` + `token::transfer(cpi_ctx, amount)?` |
| CRITICAL | CROSS-CUTTING | `presale.rs:31-47` reads `ctx.accounts.authority.lamports()` (which is the *admin/buyer* balance) and never actually transfers SOL — tokens are minted but the price is never paid; total SOL received = 0 regardless of `total_cost` | verify balance from a `buyer: Signer` account and invoke `system_program::transfer(buyer → treasury, total_cost)` before the mint CPI |
| CRITICAL | CROSS-CUTTING | `liquidity.rs:21-34` decrements `total_liquidity` and increments `sold_tokens` without performing any token or SOL transfer; `purchase_tokens` produces phantom liquidity that has no backing | remove the accounting-only path or wire the actual SPL transfer + SOL transfer in the same instruction |
| HIGH | CROSS-CUTTING | `airdrop.rs:89` uses `HashSet<Pubkey>` for `whitelisted`; Borsh cannot deterministically serialize a `HashSet` — runtime deserialization will panic or produce unstable accounts | replace with `Vec<Pubkey>` plus `contains` check, or use `BTreeSet` |
| HIGH | CROSS-CUTTING | `airdrop.rs:26` allows unbounded `whitelisted.insert(...)` against a fixed `space = 8 + 16 + 16 + 16 + 16 + 32 * 1000` (1000 entries); after that, realloc / write will fail and lock the airdrop | track `whitelisted.len()`, reject insert at max, or use `realloc` |
| HIGH | CROSS-CUTTING | `airdrop.rs:67-77` `distribute_airdrop` calls `token::transfer(CpiContext::new(...))` without `with_signer`; if the airdrop vault authority is a PDA the CPI will revert, and if the authority is a keypair the program trusts an external signer to behave | match authority design: PDA → `with_signer`; keypair → explicit role check |
| HIGH | CROSS-CUTTING | `lib.rs:26` declares `entrypoint!(process_instruction)` AND Anchor's macro-generated entrypoint — two entrypoints for the same crate causes instruction routing to silently drop; `process_instruction` is also a no-op stub | pick Anchor's generated entrypoint; remove the manual `process_instruction` and the `entrypoint!` macro |
| HIGH | CROSS-CUTTING | `lib.rs:11` declares `pub mod dev_team;` but the source file is `dev_fund.rs`; build will fail with `file not found for module 'dev_team'` | rename module to `dev_fund` (and the `pub use dev_team::*;` line at L18) |
| HIGH | CROSS-CUTTING | `token.rs:8-42` mints the full `total_supply` to a single `token_account` owned by `authority` and never revokes mint authority; the same keypair can mint unlimited additional supply at any time | either revoke mint authority after `mint_to`, or transfer mint authority to a PDA / Squads multisig |
| MEDIUM | CROSS-CUTTING | `presale.rs:18/20/22/28` uses `f64` for prices and casts `(amount as f64 * price * 1e9) as u64` — float math loses precision in financial paths | use integer math: store prices in lamports as `u64`, or use a fixed-point representation |
| MEDIUM | CROSS-CUTTING | `dev_fund.rs:31` hardcodes `24 * 3_600 * 365` seconds as "one month" — actual month lengths drift by up to ±3 days, so vesting timing is approximate by design | compute monthly seconds from a clock offset, or document the drift explicitly |
| MEDIUM | CROSS-CUTTING | `dev_fund.rs:31` `(current_time - fund_account.vesting_start_time)` is unchecked subtraction; if vesting not yet started, `current_time < vesting_start_time` underflows | `current_time.checked_sub(vesting_start_time).ok_or(ErrorCode::VestingNotStarted)?` |
| MEDIUM | CROSS-CUTTING | `dev_fund.rs:35` `elapsed_months = VESTING_PERIOD_MONTHS` silently shadows the local without updating `total_releasable` correctly; readers may miss the clamp or its consequences | hoist to a `let elapsed_months = elapsed_months.min(VESTING_PERIOD_MONTHS);` before the next line |
| MEDIUM | CROSS-CUTTING | `presale.rs:50-53` updates `total_sold` after CPI — works today, but is a classic CEI violation if any of `mint`, `mint_to`, or downstream callers become stateful (Token-2022 hooks, etc.) | move `total_sold = checked_add(amount)` to before the CPI |
| LOW | CROSS-CUTTING | `airdrop.rs:124` `whitelisted: HashSet<Pubkey>` — even if swapped for `Vec<Pubkey>`, the cost of `contains` is O(n) — at 1000 entries the per-instruction compute budget starts to bite | consider `BTreeSet` or a bitfield |
| LOW | CROSS-CUTTING | `liquidity.rs:21` accepts a `price: u64` parameter that the function never uses; dead argument invites future bugs | remove the parameter or wire it through (and rename `buyer` → actually used role) |


## Section 3 — Question 1: Missing Validations

Twelve gaps in [transfer_rewards](../assessment.txt#L65):

1. **Amount** — no zero / cap / balance check. Add `require!(amount > 0)`, `amount <= MAX_REWARD_PER_TX`, `amount <= source.amount`.
2. **Caller signer** — no `admin: Signer` + role check. Anyone invokes.
3. **Account types** — `source`, `recipient_token`, `mint` not typed. Need `Account<TokenAccount>` / `Account<Mint>` + `has_one = mint` on both token accounts.
4. **Token program** — not pinned. Add `#[account(address = anchor_spl::token::ID)]`.
5. **Vault authority ownership** — no `source.owner == vault_authority.key()` check.
6. **PDA bump** — not canonicalized / stored. Add `#[account(seeds = [b"vault", mint.key()], bump)]` and reuse stored bump in signer seeds.
7. **CPI signer seeds** — hidden behind `transfer_context()` helper. Inline `CpiContext::new_with_signer(...)` with explicit seeds.
8. **Mint compatibility** — Token-2022 transfer-hook / transfer-fee / permanent-delegate / non-transferable / confidential-transfer not rejected at vault init.
9. **Decimals** — `token::transfer` no decimals check. Use `transfer_checked` with mint + decimals.
10. **Arithmetic** — unchecked math downstream. `checked_add` / `checked_sub` on aggregations.
11. **Per-epoch / per-tx cap** — no PDA epoch counter or rent-exempt reserve.
12. **Reentrancy guard** — no state-update-before-CPI ordering, no `is_locked` flag.

Top three (amount, caller signer, account set) block deploy on their own.

---

## Section 3 — Question 2: Security Risks

**CRITICAL** (3)
- Account substitution → attacker passes own ATA as `recipient_token`, vault drains to attacker.
- CPI authority unverifiable from snippet → silent transfer-from-PDA failure or wrong authority signed.
- Insufficient balance / unchecked `amount` → DoS of claim path on empty vault.

**HIGH** (8)
- No admin signer → anyone calls, routes vault to attacker-controlled recipient.
- PDA bump off-canonical → wrong seeds → wrong authority signed.
- `transfer_context()` helper opaque → drop helper, build `CpiContext` inline, unit-test signer seeds byte-for-byte.
- Mint mismatch across source / recipient / expected → `has_one = mint` + `mint.key() == EXPECTED_MEME_MINT`.
- Token-2022 transfer-hook extension → reentrancy or DoS.
- Token-2022 transfer-fee extension → recipient credits `amount - fee`; value leak to fee recipient.
- Token-2022 permanent-delegate extension → vault drain independent of this instruction.
- Token-2022 non-transferable / confidential / transfer-gated → silent revert DoS.

**MEDIUM** (9)
- No event emission → indexers blind, audit trail missing, no proof-of-reserve.
- Vault authority design — single keypair = single point of compromise (cross-cutting; all modules share this pattern).
- Multisig gap if authority is SPL multisig → sub-threshold signers may execute.
- Reentrancy via Token-2022 hook → checks-effects-interactions violated.
- Per-recipient / per-epoch cap missing → drain in single call.
- MEV / sandwich → recipient ATA mempool-visible.
- `token::transfer` no decimals check → use `transfer_checked` with mint + decimals.
- Bare `?` on CPI leaks SPL error codes as stable oracle.
- Post-transfer invariant missing → silent Token-2022 fee drift undetected.

**LOW** (2)
- Arithmetic unchecked downstream → use `checked_add` / `checked_sub` on aggregations.
- No rate-limit / per-epoch cap → drain in single call.

---

## Section 3 — Question 3: Improvements

| # | Improvement | Description |
|---|-------------|-------------|
| 1 | **Inline CPI** | drop `transfer_context()` helper, build `CpiContext::new_with_signer(token_program, Transfer{from, to, authority}, &[&[b"vault", mint.key(), &[bump]]])` directly. |
| 2 | **Constrain account set** | typed `source: Account<TokenAccount>`, `recipient_token: Account<TokenAccount>`, `mint: Account<Mint>` + `has_one = mint` on both token accounts + `address = anchor_spl::token::ID` on `token_program`. |
| 3 | **Enforce PDA bump** | `#[account(seeds = [b"vault", mint.key().as_ref()], bump)]` on `vault_authority`, reuse stored bump in signer seeds. |
| 4 | **Add admin signer + role check** | `admin: Signer` + `#[access_control(admin_is_authorized(&ctx.accounts.admin))]` resolving to Squads multisig PDA check. |
| 5 | **Validate amount** | `require!(amount > 0 && amount <= MAX_REWARD_PER_TX && amount <= source.amount, ErrorCode::InvalidAmount)`. |
| 6 | **Switch to `transfer_checked`** | passes mint + decimals, blocks mint substitution, surfaces decimal mismatches. |
| 7 | **Map errors** | `.map_err(\|_\| error!(ErrorCode::TokenTransferFailed))?` to hide SPL oracle. |
| 8 | **Emit event** | `#[event] RewardsTransferred { recipient, mint, vault, amount, ts }` before / after CPI. |
| 9 | **Reload + assert invariant** | `source.reload()?` + `recipient_token.reload()?`, assert `destination.amount - prev >= amount` (catches Token-2022 fee drift). |
| 10 | **Reject hostile Token-2022 extensions at vault init** | transfer-hook, transfer-fee, permanent-delegate, non-transferable, confidential-transfer. |
| 11 | **Per-epoch cap + rent-exempt reserve** | stored on PDA, enforced on each transfer. |
| 12 | **Checks-effects-interactions** | update reward accounting state before CPI (or refuse hook mints entirely). |
| 13 | **Squads multisig** | as `admin` role, replacing single-key compromise risk. |
| 14 | **Vault init instruction** | (separate): validate mint extension set, seed PDA with bump, create vault token account owned by PDA. |
| 15 | **Unit test signer seeds** | assert `signer_seeds` byte-for-byte match `seeds + bump` in CI. |
| 16 | **Token-2022 mint allowlist** | known-good mints only, reject unknown extensions. |
| 17 | **Audit + bug bounty** | before mainnet (Neodyme, OtterSec, Trail of Bits). |

---

## Patched Implementation (draft)

> Self-contained drop-in program (init + transfer + Token-2022 mint gating). Companion with full tests → [section3-implementation.md](section3-implementation.md).

```rust
use anchor_lang::prelude::*;
use anchor_spl::token_2022::{
    self,
    spl_token_2022::{
        extension::{ExtensionType, StateWithExtensions},
        state::Mint as SplMintState,
    },
    Token2022,
};
use anchor_spl::token::{TokenAccount, TransferChecked};

declare_id!("MemoXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

// ---------- Constants ----------

pub const MAX_REWARD_PER_TX: u64 = 1_000_000_000_000;
pub const RENT_EXEMPT_RESERVE: u64 = 2_039_280;
pub const EXPECTED_MEME_MINT: Pubkey = pubkey!("MemoSoMeMeMeMeMeMeMeMeMeMeMeMeMeMeMe");

// ---------- Events ----------

#[event]
pub struct VaultInitialized {
    pub vault: Pubkey,
    pub mint: Pubkey,
    pub admin: Pubkey,
    pub ts: i64,
}

#[event]
pub struct RewardsTransferred {
    pub recipient: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub amount: u64,
    pub ts: i64,
}

// ---------- Errors ----------

#[error_code]
pub enum ErrorCode {
    #[msg("amount must be > 0 and <= vault balance")]
    InvalidAmount,
    #[msg("amount exceeds per-tx cap")]
    RewardCap,
    #[msg("unauthorized admin")]
    Unauthorized,
    #[msg("SPL token transfer failed")]
    TokenTransferFailed,
    #[msg("vault balance below rent-exempt reserve")]
    VaultBelowReserve,
    #[msg("mint has transfer-hook extension")]
    IncompatibleMint,
}

// ---------- Accounts ----------

#[account]
pub struct VaultAuthority {
    pub bump: u8,
    pub admin: Pubkey,
    pub mint: Pubkey,
}

impl VaultAuthority {
    pub const LEN: usize = 8 + 1 + 32 + 32;
}

// ---------- Init Vault ----------

#[derive(Accounts)]
pub struct InitVault<'info> {
    pub mint: AccountInfo<'info>,

    #[account(
        init,
        payer = admin,
        space = VaultAuthority::LEN,
        seeds = [b"vault", mint.key().as_ref()],
        bump,
    )]
    pub vault_authority: Account<'info, VaultAuthority>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[access_control(admin_is_authorized(&ctx.accounts.admin, &ctx.accounts.vault_authority))]
pub fn init_vault(ctx: Context<InitVault>) -> Result<()> {
    // Reject incompatible Token-2022 extensions before persisting the vault.
    let mint_info = ctx.accounts.mint.to_account_info();
    let mint_data = mint_info.try_borrow_data()?;
    let mint_state = StateWithExtensions::<SplMintState>::unpack(&mint_data)?;

    for i in 0..mint_state.get_extension_count() {
        let ext_type = mint_state.try_get_extension_type(i)?;
        require!(
            !is_incompatible_extension(ext_type),
            ErrorCode::IncompatibleMint
        );
    }

    let vault_authority = &mut ctx.accounts.vault_authority;
    vault_authority.bump = ctx.bumps.vault_authority;
    vault_authority.admin = ctx.accounts.admin.key();
    vault_authority.mint = ctx.accounts.mint.key();

    let clock = Clock::get()?;
    emit!(VaultInitialized {
        vault: ctx.accounts.vault_authority.key(),
        mint: ctx.accounts.mint.key(),
        admin: ctx.accounts.admin.key(),
        ts: clock.unix_timestamp,
    });

    Ok(())
}

fn is_incompatible_extension(ext_type: ExtensionType) -> bool {
    matches!(
        ext_type,
        ExtensionType::TransferHook
            | ExtensionType::TransferFeeConfig
            | ExtensionType::PermanentDelegate
            | ExtensionType::NonTransferable
            | ExtensionType::ConfidentialTransferAccount
            | ExtensionType::ConfidentialTransferMint
    )
}

// ---------- Transfer Rewards ----------

#[derive(Accounts)]
pub struct TransferRewards<'info> {
    pub mint: AccountInfo<'info>,

    #[account(
        mut,
        constraint = source.mint == mint.key() @ ErrorCode::Unauthorized,
        constraint = source.owner == vault_authority.key() @ ErrorCode::Unauthorized,
    )]
    pub source: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = recipient_token.mint == mint.key() @ ErrorCode::Unauthorized,
    )]
    pub recipient_token: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"vault", mint.key().as_ref()],
        bump = vault_authority.bump,
        constraint = vault_authority.mint == mint.key() @ ErrorCode::Unauthorized,
    )]
    pub vault_authority: Account<'info, VaultAuthority>,

    #[account(
        address = anchor_spl::token_2022::ID @ ErrorCode::Unauthorized,
    )]
    pub token_program: Program<'info, Token2022>,

    #[account(mut)]
    pub admin: Signer<'info>,
}

#[access_control(admin_is_authorized(&ctx.accounts.admin, &ctx.accounts.vault_authority))]
pub fn transfer_rewards(ctx: Context<TransferRewards>, amount: u64) -> Result<()> {
    // ---- Checks ----
    require!(amount > 0, ErrorCode::InvalidAmount);
    require!(amount <= MAX_REWARD_PER_TX, ErrorCode::RewardCap);
    require!(
        ctx.accounts.source.amount >= amount,
        ErrorCode::InvalidAmount
    );
    require!(
        ctx.accounts.source.amount - amount >= RENT_EXEMPT_RESERVE,
        ErrorCode::VaultBelowReserve
    );

    // ---- Effects ----
    // (in a richer program: deduct from a per-recipient / per-epoch cap PDA here)

    // ---- Interactions ----
    let mint_key = ctx.accounts.mint.key();
    let bump = ctx.accounts.vault_authority.bump;
    let signer_seeds: &[&[&[u8]]] = &[&[b"vault", mint_key.as_ref(), &[bump]]];

    let decimals = ctx.accounts.recipient_token.decimals;

    let cpi_accounts = TransferChecked {
        from: ctx.accounts.source.to_account_info(),
        to: ctx.accounts.recipient_token.to_account_info(),
        authority: ctx.accounts.vault_authority.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
    };
    let cpi_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
    )
    .with_signer(signer_seeds);

    anchor_spl::token_2022::transfer_checked(cpi_ctx, amount, decimals)
        .map_err(|_| error!(ErrorCode::TokenTransferFailed))?;

    // ---- Post-condition invariant ----
    let pre_dest = ctx.accounts.recipient_token.amount;
    ctx.accounts.source.reload()?;
    ctx.accounts.recipient_token.reload()?;
    let dest_delta = ctx
        .accounts
        .recipient_token
        .amount
        .checked_sub(pre_dest)
        .ok_or(error!(ErrorCode::TokenTransferFailed))?;
    require!(dest_delta >= amount, ErrorCode::TokenTransferFailed);

    let clock = Clock::get()?;
    emit!(RewardsTransferred {
        recipient: ctx.accounts.recipient_token.key(),
        mint: ctx.accounts.mint.key(),
        vault: ctx.accounts.source.key(),
        amount,
        ts: clock.unix_timestamp,
    });

    Ok(())
}

// ---------- Authorization ----------
// Strict role check: caller must equal the admin stored on the VaultAuthority PDA.
// Replace with Squads multisig PDA check for production.

fn admin_is_authorized(admin: &Signer, vault_authority: &Account<VaultAuthority>) -> Result<()> {
    require!(
        admin.key() == vault_authority.admin,
        ErrorCode::Unauthorized
    );
    Ok(())
}
```

### Realistic anchor-spl::token::transfer form (for reference)

```rust
// The 3-argument `token::transfer(program, accounts, amount)` form seen in
// dev_fund.rs:54-58 and presale.rs:43-47 does not match Anchor 0.30 — use this:
token::transfer(
    CpiContext::new(token_program.to_account_info(), cpi_accounts),
    amount,
)?;
```

> Full program + tests → [section3-implementation.md](section3-implementation.md).

---

## Section 3 — Question 4: Tests

Full test stubs across all categories → [docs/section3-implementation.md §Test Suite](docs/section3-implementation.md).

**Compact test matrix**

| # | Case | Expect |
|---|------|--------|
| 1 | Admin transfers valid amount | source debits, recipient credits, event emitted |
| 2 | amount = 0 | InvalidAmount |
| 3 | amount > MAX_REWARD_PER_TX | RewardCap |
| 4 | amount > source.amount | InvalidAmount |
| 5 | Non-admin signer | Unauthorized (after admin_is_authorized wired) |
| 6 | Wrong mint on recipient_token | constraint fail |
| 7 | Wrong mint on source | constraint fail |
| 8 | Token program not SPL | constraint fail |
| 9 | PDA bump off-canonical | constraint fail |
| 10 | Vault with transfer-fee mint | `IncompatibleMint` at init — fee-bearing mints rejected |
| 11 | Hook-bearing mint at init | `IncompatibleMint` at init |
| 12 | Post-transfer invariant (Q3 #9) | dest_delta >= amount asserted via `recipient_token.reload()` |
| 13 | CPI signer seeds unit test (Q3 #15) | signer_seeds byte-for-byte == `[b"vault", mint, &[bump]]` |
| 14 | Fuzz 1k random amounts | no panic, no overflow, typed errors only |
| 15 | Event struct asserted via anchor::Event | match fields |

**E2E (devnet / mainnet pre-launch)**
- Full happy path with Squads multisig as admin.
- Indexer consumes events, parses fields correctly.
- Treasury remains rent-exempt after 1k transfers.

Full test stub in [docs/section3-implementation.md §Test Suite](docs/section3-implementation.md).
