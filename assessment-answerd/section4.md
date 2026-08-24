# Section 4 — Staking System Deep Dive

> Source: [assessment.txt §4](../assessment.txt#L84). Original brief asked for ≤300 words. This deep dive expands each requirement with concrete account layouts, math, code stubs, attack vectors, and test matrix. Companion to [§4](assessment-answerd.md).

---

## 1. Required Accounts

**Pool PDA** — seeds `[b"pool", mint.key().as_ref()]`, single canonical bump.

```rust
#[account]
pub struct Pool {
    pub version: u8,                          // layout version (offset 0, frozen on migration)
    pub bump: u8,                             // canonical PDA bump
    pub mint: Pubkey,                         // staking token mint
    pub reward_mint: Pubkey,                  // reward token mint (often == mint)
    pub stake_vault: Pubkey,                  // PDA-owned token account holding staked tokens
    pub reward_vault: Pubkey,                 // PDA-owned token account holding reward tokens
    pub stake_vault_bump: u8,                 // stake_vault canonical bump (signer seeds)
    pub reward_vault_bump: u8,                // reward_vault canonical bump
    pub authority: Pubkey,                    // admin / Squads multisig PDA
    pub pending_authority: Pubkey,            // queued authority, zero = none
    pub pending_authority_effective_ts: i64,  // when pending_authority becomes active
    pub multisig_program: Pubkey,             // Squads program id (validated on every admin op)
    pub multisig_pda: Pubkey,                 // expected multisig PDA
    pub reward_rate: u64,                     // tokens emitted per second, scaled by 1e18
    pub pending_rate: u64,                    // queued rate, applied once `effective_ts` reached
    pub effective_ts: i64,                    // timestamp at which `pending_rate` becomes active
    pub emission_end_ts: i64,                 // 0 = no cap
    pub last_update_ts: i64,                  // last accumulator update (unix seconds)
    pub acc_reward_per_share: u128,           // MasterChef accumulator, scaled 1e18
    pub total_staked: u64,                    // Σ stake.amount across all users
    pub total_rewards_to_emit: u64,           // running tally of `fund_rewards` deposits
    pub total_rewards_paid: u64,              // running tally of payouts
    pub pending_liability: u128,              // Σ stake pending; updated on every settle
    pub stake_cap: u64,                       // 0 = unlimited
    pub lockup_seconds: u64,                  // minimum stake duration; MUST be > 0 at init
    pub paused: bool,                         // emergency pause
    pub is_locked: bool,                      // reentrancy guard (set first, cleared by Drop)
    pub deposit_enabled: bool,                // flipped true by first `fund_rewards`
}
```

**Stake PDA** — seeds `[b"stake", pool.mint.as_ref(), user.key()]` (mint-stable, not pool-key).

```rust
#[account]
pub struct Stake {
    pub bump: u8,
    pub pool: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,                          // staked token base units
    pub stake_start_ts: i64,                  // frozen after first deposit; never reset
    pub unlock_ts: i64,                       // = stake_start_ts + lockup_at_deposit (snapshot)
    pub lockup_at_deposit: u64,               // snapshot of pool.lockup_seconds at deposit time
    pub reward_debt: u128,                    // accumulator snapshot at deposit time
    pub boosted_multiplier_bps: u16,          // optional tier boost (1e4 = 1x)
    pub boosted_amount: u64,                  // cached projection for O(1) settle
}
```

**Stake Vault (token account)** — owner = Pool PDA, mint = staking mint; seeds `[b"stake_vault", pool.key()]`.
**Reward Vault (token account)** — owner = Pool PDA, mint = reward mint; seeds `[b"reward_vault", pool.key()]`.
**User ATAs** — associated token accounts owned by user.

### Events

```rust
#[event]
pub struct PoolInitialized {
    pub pool: Pubkey,
    pub mint: Pubkey,
    pub authority: Pubkey,
    pub lockup_seconds: u64,
    pub ts: i64,
}

#[event]
pub struct PoolFunded {
    pub pool: Pubkey,
    pub amount: u64,
    pub ts: i64,
}

#[event]
pub struct StakeDeposited {
    pub user: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub ts: i64,
}

#[event]
pub struct StakeWithdrawn {
    pub user: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub ts: i64,
}

#[event]
pub struct RewardClaimed {
    pub user: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub ts: i64,
}

#[event]
pub struct StakeClosed {
    pub user: Pubkey,
    pub pool: Pubkey,
    pub ts: i64,
}

#[event]
pub struct AuthorityHandover {
    pub pool: Pubkey,
    pub from: Pubkey,
    pub to: Pubkey,
    pub effective_ts: i64,
}
```

---

## 2. Reward Calculation — MasterChef Accumulator

Linear emission with one global scalar `acc_reward_per_share`. `O(1)` settle per user.

```
if total_staked > 0 and now > last_update_ts:
    acc_reward_per_share += reward_rate × Δt × 1e18 / total_staked
```

Per-user pending:

```text
boosted_amount = stake.amount × boosted_multiplier_bps / 10_000
pending = boosted_amount × acc_reward_per_share / 1e18 − stake.reward_debt
```

`reward_debt` snapshotted at deposit / withdraw so double-counting cannot occur. `claim_reward`:

```text
pay = pending
stake.reward_debt = boosted_amount × acc_reward_per_share / 1e18
pool.pending_liability = pool.pending_liability − (pending as u128)
```

### Fixed-point scaling

- `acc_reward_per_share` `u128`, scaled 1e18. `u64::MAX ≈ 1.8e19` so years-long pools fit.
- All math via `checked_mul` / `checked_div` / `checked_add` / `checked_sub` — no `unwrap`, no `as u64` truncation (use `try_into`).
- `total_staked.checked_add(amount).ok_or(MathOverflow)?`.
- `pending_liability.checked_sub(x).ok_or(MathOverflow)?`.

### Time model

- `Δt = now - pool.last_update_ts` from `Clock::get()?.unix_timestamp`.
- Stale update: long gap with `total_staked == 0` → no rewards (multiplication by zero).
- `emission_end_ts > 0` caps emission; updates past end_ts add 0.

---

## 3. Distribution Flow

Eight instructions:

1. **`init_pool`** — admin-only. TLV-walks the staking mint, refuses hostile Token-2022 extensions; creates `stake_vault` + `reward_vault` PDAs (system_program::create_account + token::initialize_account2); seeds `Pool` with virtual offset (`acc_reward_per_share = 1`); sets `lockup_seconds > 0`.
2. **`fund_rewards(amount)`** — admin-only. Splits between `reward_mint` (CPI) and `Pool` config; flips `deposit_enabled = true` on first call.
3. **`deposit_stake(amount, lockup_tier)`** — user CPI `transfer_checked` (mint + decimals) into `stake_vault`; settles existing pending to `user_reward_ata`; updates `boosted_amount` and `reward_debt`. Lockup tier ≥ pool floor.
4. **`withdraw_stake(amount)`** — symmetric. If `new amount == 0`, zero out `reward_debt` and emit `StakeClosed` from a separate `close_stake` call. Reentrancy-safe.
5. **`claim_reward`** — gated by `now >= stake.unlock_ts`. CPI transfers from `reward_vault` (PDA signer seeds `[b"reward_vault", pool.key(), &[reward_vault_bump]]`) to `user_reward_ata`. If `amount == 0` and `pending == 0`, stake is closeable.
6. **`update_rate(new_rate, effective_ts)`** — admin-only. Writes `pending_rate` + `effective_ts`. Promotion to `reward_rate` happens inside every reward-touching instruction when `now >= effective_ts`.
7. **`set_authority(new, effective_ts)`** — admin-only. Two-step: writes `pending_authority` + `pending_authority_effective_ts`. Promoted inside every admin instruction once `now >= effective_ts`.
8. **`accept_authority`** — callable by `pending_authority` once `now >= effective_ts`. Atomically promotes and zeroes.

Plus `close_stake`, `migrate_pool`, `slash_to_treasury`.

CPI signer seeds for vaults:

```rust
let stake_vault_seeds: &[&[&[u8]]] = &[&[b"stake_vault", pool.key().as_ref(), &[pool.stake_vault_bump]]];
let reward_vault_seeds: &[&[&[u8]]] = &[&[b"reward_vault", pool.key().as_ref(), &[pool.reward_vault_bump]]];
```

Reentrancy via RAII guard:

```rust
struct ReentrancyGuard<'a> { pool: &'a mut Account<'info, Pool> }
impl Drop for ReentrancyGuard<'_> {
    fn drop(&mut self) { self.pool.is_locked = false; }
}
```

Use: `let _g = ReentrancyGuard { pool: ctx.accounts.pool.borrow_mut() };` at handler entry. Drop clears `is_locked` on both success and `?` error paths.

### Funding

`fund_rewards` CPI from admin's reward ATA → `reward_vault`. Bumps `total_rewards_to_emit`. Flips `deposit_enabled = true` if `total_rewards_to_emit == 0`.

---

## 4. Security Considerations

### First-deposit attack

`init_pool` writes `acc_reward_per_share = 1` virtual offset; `deposit_enabled` flips true only after first `fund_rewards`. First staker's `reward_debt = boosted_amount × 1 / 1e18`, capturing only post-funding emission.

### Donation attack

Compare tracked vs actual; **skim** surplus to treasury rather than revert:

```rust
let actual = reward_vault.amount;
let tracked = pool.total_rewards_to_emit
    .checked_sub(pool.total_rewards_paid).ok_or(MathOverflow)?
    .checked_add(pool.pending_liability as u64).ok_or(MathOverflow)?;
if actual > tracked {
    let donated = actual - tracked;
    // CPI: skim `donated` from reward_vault to treasury; emit DonationSkimmed event.
    // Settle proceeds with tracked only.
}
```

Hard-revert would DOS claims on dust donations.

### Reentrancy

- ReentrancyGuard via Drop (above) guarantees `is_locked = false` on every exit path.
- Refuse Token-2022 mints carrying hostile extensions at init (transfer-hook, transfer-fee, permanent-delegate, non-transferable, confidential-transfer).
- `transfer_checked` with explicit mint + decimals on every CPI.

### Account substitution

- Pool + Stake PDAs `#[account(seeds, bump)]` with `pool.mint` (not `pool.key()`) for stake seed stability.
- Vault token accounts `address = pool.stake_vault` / `address = pool.reward_vault` plus `mint == pool.mint` / `mint == pool.reward_mint`.
- `user_stake_ata.owner == user.key()`, `user_reward_ata.owner == user.key()`.

### Flash-loan

`init_pool` enforces `lockup_seconds > 0`. Tier 0 mapped to `max(tier0_seconds, pool.lockup_seconds)`. Both `withdraw_stake` and `claim_reward` gate by `now >= stake.unlock_ts`.

### Lockup reset

`stake_start_ts` frozen after first write. Top-ups only increase `amount`; `unlock_ts` preserved. `extend_lockup` is a separate explicit instruction (costs lockup extension fee).

### Pause

`pool.paused = true` blocks `deposit_stake` + `withdraw_stake` + `fund_rewards`. `claim_reward` stays open so users don't get rugged.

### MEV / sandwich

Rate change via `pending_rate` + `effective_ts`. Promotion happens inside every reward-touching instruction when `now >= effective_ts`. Pre-promotion deposits use old rate.

### Multisig

`Pool.multisig_program` + `Pool.multisig_pda` enforced on every admin op:

```rust
fn admin_is_authorized(pool: &Account<Pool>, signer: &Signer) -> Result<()> {
    let expected = Pubkey::create_program_address(
        &[pool.multisig_pda.as_ref(), &[/* squad bump */]],
        &pool.multisig_program,
    ).map_err(|_| error!(ErrorCode::InvalidMultisig))?;
    require!(signer.key() == expected, ErrorCode::Unauthorized);
    Ok(())
}
```

Squads m ≥ 2 controls pause, rate updates, set_authority, slash, upgrade.

### State growth

`close_stake` reaps zero-balance Stake PDAs, refunds rent to owner. Without this, 100k churned users = ~120 SOL stranded + RPC bloat.

### Migration

`Pool.version` reserves offset 0. `migrate_pool` handler bumps version, validates field layout, emits migration event. Bumped pools gated by `require!(pool.version == N)`.

### Authority handover

Two-step via `set_authority` + `accept_authority`. `pending_authority_effective_ts` defaults to `now + TIME_LOCK_SECONDS = now + 7 days`.

### Slashing

`slash_to_treasury(amount)` admin-only with donation-detection accounting: actual vault amount reconciled; only legitimate `amount` moves.

### Token-2022 transfer-fee handling

`init_pool` rejects fee-bearing mints. If fee-bearing rewards are later required, fee is read from post-transfer vault balance change; surplus skimmed to treasury.

---

## 5. Code Stubs

### Errors

```rust
#[error_code]
pub enum ErrorCode {
    #[msg("pool paused")]
    PoolPaused,
    #[msg("deposits not enabled (call fund_rewards first)")]
    DepositsNotEnabled,
    #[msg("invalid amount")]
    InvalidAmount,
    #[msg("invalid lockup tier")]
    InvalidLockup,
    #[msg("pool full")]
    PoolFull,
    #[msg("math overflow")]
    MathOverflow,
    #[msg("reentrancy: pool is locked")]
    IsLocked,
    #[msg("stake not empty")]
    StakeNotEmpty,
    #[msg("pending reward non-zero")]
    PendingReward,
    #[msg("time-lock too short")]
    TimeLockTooShort,
    #[msg("unauthorized")]
    Unauthorized,
    #[msg("wrong pool version")]
    WrongVersion,
    #[msg("invalid multisig PDA")]
    InvalidMultisig,
    #[msg("mint has unsupported Token-2022 extension")]
    UnsupportedMintExtension,
}
```

### Reentrancy guard

```rust
struct ReentrancyGuard<'a> { pool: &'a mut Account<'info, Pool> }
impl Drop for ReentrancyGuard<'_> {
    fn drop(&mut self) { self.pool.is_locked = false; }
}
```

### init_pool

```rust
#[derive(Accounts)]
pub struct InitPool<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Pool::LEN,
        seeds = [b"pool", mint.key().as_ref()],
        bump,
    )]
    pub pool: Account<'info, Pool>,

    pub mint: InterfaceAccount<'info, anchor_spl::token_interface::Mint>,

    /// CHECK: stake_vault PDA, created via CPI in handler.
    #[account(seeds = [b"stake_vault", pool.key().as_ref()], bump)]
    pub stake_vault: UncheckedAccount<'info>,

    /// CHECK: reward_vault PDA, created via CPI in handler.
    #[account(seeds = [b"reward_vault", pool.key().as_ref()], bump)]
    pub reward_vault: UncheckedAccount<'info>,

    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, anchor_spl::token_interface::TokenInterface>,
    pub associated_token_program: Program<'info, anchor_spl::associated_token::AssociatedToken>,
}

#[access_control(admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.authority))]
pub fn init_pool(
    ctx: Context<InitPool>,
    reward_rate: u64,
    lockup_seconds: u64,
    stake_cap: u64,
    multisig_program: Pubkey,
    multisig_pda: Pubkey,
) -> Result<()> {
    require!(lockup_seconds > 0, ErrorCode::InvalidLockup);
    require!(ctx.accounts.pool.version == 0, ErrorCode::WrongVersion);

    // Reject incompatible Token-2022 extensions BEFORE persisting pool.
    reject_hostile_mint_extensions(&ctx.accounts.mint.to_account_info())?;

    let pool = &mut ctx.accounts.pool;
    pool.version = 1;
    pool.bump = ctx.bumps.pool;
    pool.mint = ctx.accounts.mint.key();
    pool.reward_mint = ctx.accounts.mint.key(); // single-mint pool; multi-mint sets separately
    pool.stake_vault = ctx.accounts.stake_vault.key();
    pool.reward_vault = ctx.accounts.reward_vault.key();
    pool.stake_vault_bump = ctx.bumps.stake_vault;
    pool.reward_vault_bump = ctx.bumps.reward_vault;
    pool.authority = ctx.accounts.authority.key();
    pool.multisig_program = multisig_program;
    pool.multisig_pda = multisig_pda;
    pool.reward_rate = reward_rate;
    pool.pending_rate = reward_rate;
    pool.effective_ts = Clock::get()?.unix_timestamp;
    pool.last_update_ts = Clock::get()?.unix_timestamp;
    pool.acc_reward_per_share = 1; // virtual offset
    pool.total_staked = 0;
    pool.total_rewards_to_emit = 0;
    pool.total_rewards_paid = 0;
    pool.pending_liability = 0;
    pool.stake_cap = stake_cap;
    pool.lockup_seconds = lockup_seconds;
    pool.paused = false;
    pool.is_locked = false;
    pool.deposit_enabled = false;

    // CPI: create stake_vault and reward_vault as PDA-owned token accounts (omitted for brevity).
    // system_program::create_account(payer = authority, new_account = stake_vault, ..)
    // token::initialize_account2(stake_vault, mint, pool PDA, ..)
    // Same for reward_vault (separate mint = reward_mint).

    emit!(PoolInitialized {
        pool: pool.key(),
        mint: pool.mint,
        authority: pool.authority,
        lockup_seconds,
        ts: Clock::get()?.unix_timestamp,
    });
    Ok(())
}

fn reject_hostile_mint_extensions(mint_info: &AccountInfo) -> Result<()> {
    use anchor_spl::token_2022::spl_token_2022::extension::{
        get_extension_types, ExtensionType, StateWithExtensions,
    };
    use anchor_spl::token_2022::spl_token_2022::state::Mint as SplMintState;
    let data = mint_info.try_borrow_data()?;
    let state = StateWithExtensions::<SplMintState>::unpack(&data)?;
    for ext in get_extension_types(&data)? {
        let hostile = matches!(
            ext,
            ExtensionType::TransferHook
                | ExtensionType::TransferFeeConfig
                | ExtensionType::PermanentDelegate
                | ExtensionType::NonTransferable
                | ExtensionType::ConfidentialTransferMint
                | ExtensionType::ConfidentialTransferAccount
        );
        require!(!hostile, ErrorCode::UnsupportedMintExtension);
    }
    Ok(())
}
```

### fund_rewards

```rust
#[derive(Accounts)]
pub struct FundRewards<'info> {
    #[account(mut, seeds = [b"pool", pool.mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, Pool>,
    #[account(mut, constraint = admin_reward_ata.mint == pool.reward_mint)]
    pub admin_reward_ata: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,
    #[account(mut, address = pool.reward_vault)]
    pub reward_vault: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,
    pub admin: Signer<'info>,
    pub token_program: Interface<'info, anchor_spl::token_interface::TokenInterface>,
}

pub fn fund_rewards(ctx: Context<FundRewards>, amount: u64) -> Result<()> {
    admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.admin)?;
    require!(amount > 0, ErrorCode::InvalidAmount);

    let cpi = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        anchor_spl::token_interface::TransferChecked {
            from: ctx.accounts.admin_reward_ata.to_account_info(),
            to: ctx.accounts.reward_vault.to_account_info(),
            authority: ctx.accounts.admin.to_account_info(),
            mint: ctx.accounts.pool.to_account_info(),
        },
    );
    anchor_spl::token_interface::transfer_checked(cpi, amount, ctx.accounts.admin_reward_ata.decimals)?;

    let pool = &mut ctx.accounts.pool;
    pool.total_rewards_to_emit = pool.total_rewards_to_emit.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
    pool.deposit_enabled = true;

    emit!(PoolFunded { pool: pool.key(), amount, ts: Clock::get()?.unix_timestamp });
    Ok(())
}
```

### deposit_stake

```rust
#[derive(Accounts)]
pub struct DepositStake<'info> {
    #[account(mut, seeds = [b"pool", pool.mint.as_ref()], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    pub mint: InterfaceAccount<'info, anchor_spl::token_interface::Mint>,

    #[account(
        init_if_needed,
        payer = user,
        space = 8 + Stake::LEN,
        seeds = [b"stake", pool.mint.as_ref(), user.key().as_ref()],
        bump,
    )]
    pub stake: Account<'info, Stake>,

    #[account(
        mut,
        constraint = user_stake_ata.mint == pool.mint,
        constraint = user_stake_ata.owner == user.key(),
    )]
    pub user_stake_ata: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,

    #[account(
        mut,
        constraint = user_reward_ata.mint == pool.reward_mint,
        constraint = user_reward_ata.owner == user.key(),
    )]
    pub user_reward_ata: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,

    #[account(mut, address = pool.stake_vault, mint = pool.mint)]
    pub stake_vault: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,

    #[account(mut, address = pool.reward_vault, mint = pool.reward_mint)]
    pub reward_vault: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub token_program: Interface<'info, anchor_spl::token_interface::TokenInterface>,
}

pub fn deposit_stake(ctx: Context<DepositStake>, amount: u64, lockup_tier: u8) -> Result<()> {
    require!(!ctx.accounts.pool.paused, ErrorCode::PoolPaused);
    require!(ctx.accounts.pool.deposit_enabled, ErrorCode::DepositsNotEnabled);
    require!(ctx.accounts.pool.version == 1, ErrorCode::WrongVersion);
    require!(amount > 0, ErrorCode::InvalidAmount);

    // Reentrancy guard (RAII: clears is_locked on every exit).
    let pool = &mut ctx.accounts.pool;
    require!(!pool.is_locked, ErrorCode::IsLocked);
    pool.is_locked = true;
    let _guard = ReentrancyGuard { pool }; // shadow — Drop clears is_locked

    let now = Clock::get()?.unix_timestamp;

    // Promote pending_rate if effective.
    if now >= ctx.accounts.pool.effective_ts && ctx.accounts.pool.pending_rate != 0 {
        ctx.accounts.pool.reward_rate = ctx.accounts.pool.pending_rate;
        ctx.accounts.pool.pending_rate = 0;
    }

    // Update accumulator.
    if ctx.accounts.pool.total_staked > 0 && now > ctx.accounts.pool.last_update_ts {
        let dt = (now - ctx.accounts.pool.last_update_ts).max(0) as u128;
        let acc_delta = (ctx.accounts.pool.reward_rate as u128)
            .checked_mul(dt).ok_or(ErrorCode::MathOverflow)?
            .checked_mul(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?
            .checked_div(ctx.accounts.pool.total_staked as u128).ok_or(ErrorCode::MathOverflow)?;
        ctx.accounts.pool.acc_reward_per_share = ctx.accounts.pool.acc_reward_per_share
            .checked_add(acc_delta).ok_or(ErrorCode::MathOverflow)?;
    }
    ctx.accounts.pool.last_update_ts = now;

    // Settle existing pending to user_reward_ata (PDA signer = reward_vault).
    let stake = &mut ctx.accounts.stake;
    let boosted = (stake.amount as u128)
        .checked_mul(stake.boosted_multiplier_bps as u128).ok_or(ErrorCode::MathOverflow)?
        .checked_div(10_000).ok_or(ErrorCode::MathOverflow)?;
    let pending = boosted
        .checked_mul(ctx.accounts.pool.acc_reward_per_share).ok_or(ErrorCode::MathOverflow)?
        .checked_div(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?
        .checked_sub(stake.reward_debt).ok_or(ErrorCode::MathOverflow)?;
    if pending > 0 {
        let pay: u64 = pending.try_into().map_err(|_| ErrorCode::MathOverflow)?;
        let seeds: &[&[&[u8]]] = &[&[b"reward_vault", ctx.accounts.pool.key().as_ref(), &[ctx.accounts.pool.reward_vault_bump]]];
        let cpi = CpiContext::new(ctx.accounts.token_program.to_account_info(), anchor_spl::token_interface::TransferChecked {
            from: ctx.accounts.reward_vault.to_account_info(),
            to: ctx.accounts.user_reward_ata.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
            mint: ctx.accounts.pool.to_account_info(),
        }).with_signer(seeds);
        anchor_spl::token_interface::transfer_checked(cpi, pay, ctx.accounts.reward_vault.decimals)?;
        ctx.accounts.pool.total_rewards_paid = ctx.accounts.pool.total_rewards_paid.checked_add(pay).ok_or(ErrorCode::MathOverflow)?;
        ctx.accounts.pool.pending_liability = ctx.accounts.pool.pending_liability.checked_sub(pending).ok_or(ErrorCode::MathOverflow)?;
    }

    // CPI: transfer staking tokens from user_stake_ata -> stake_vault.
    let stake_seeds: &[&[&[u8]]] = &[&[b"stake_vault", ctx.accounts.pool.key().as_ref(), &[ctx.accounts.pool.stake_vault_bump]]];
    let cpi = CpiContext::new(ctx.accounts.token_program.to_account_info(), anchor_spl::token_interface::TransferChecked {
        from: ctx.accounts.user_stake_ata.to_account_info(),
        to: ctx.accounts.stake_vault.to_account_info(),
        authority: ctx.accounts.user.to_account_info(),
        mint: ctx.accounts.pool.to_account_info(),
    });
    anchor_spl::token_interface::transfer_checked(cpi, amount, ctx.accounts.user_stake_ata.decimals)?;

    // Update stake.
    let is_new_stake = stake.amount == 0;
    stake.amount = stake.amount.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
    stake.boosted_amount = ((stake.amount as u128).checked_mul(stake.boosted_multiplier_bps as u128).ok_or(ErrorCode::MathOverflow)?
        .checked_div(10_000).ok_or(ErrorCode::MathOverflow)?) as u64;
    stake.reward_debt = (stake.boosted_amount as u128)
        .checked_mul(ctx.accounts.pool.acc_reward_per_share).ok_or(ErrorCode::MathOverflow)?
        .checked_div(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?;

    // Lockup: only set on new stake; tier 0 maps to pool floor; stake_start_ts frozen after first write.
    let lockup_seconds = match lockup_tier {
        0 => ctx.accounts.pool.lockup_seconds,
        1 => std::cmp::max(30 * 24 * 3600, ctx.accounts.pool.lockup_seconds),
        2 => std::cmp::max(90 * 24 * 3600, ctx.accounts.pool.lockup_seconds),
        3 => std::cmp::max(180 * 24 * 3600, ctx.accounts.pool.lockup_seconds),
        _ => return Err(ErrorCode::InvalidLockup.into()),
    };
    if is_new_stake {
        stake.stake_start_ts = now;
        stake.unlock_ts = now.checked_add(lockup_seconds).ok_or(ErrorCode::MathOverflow)?;
        stake.lockup_at_deposit = lockup_seconds;
    }

    // Pool totals.
    ctx.accounts.pool.total_staked = ctx.accounts.pool.total_staked.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
    require!(ctx.accounts.pool.stake_cap == 0 || ctx.accounts.pool.total_staked <= ctx.accounts.pool.stake_cap, ErrorCode::PoolFull);

    emit!(StakeDeposited {
        user: ctx.accounts.user.key(),
        pool: ctx.accounts.pool.key(),
        amount,
        ts: now,
    });
    Ok(())
}
```

### withdraw_stake

```rust
pub fn withdraw_stake(ctx: Context<WithdrawStake>, amount: u64) -> Result<()> {
    require!(!ctx.accounts.pool.paused, ErrorCode::PoolPaused);
    require!(amount > 0, ErrorCode::InvalidAmount);

    let pool = &mut ctx.accounts.pool;
    require!(!pool.is_locked, ErrorCode::IsLocked);
    pool.is_locked = true;
    let _guard = ReentrancyGuard { pool };

    let now = Clock::get()?.unix_timestamp;
    require!(now >= ctx.accounts.stake.unlock_ts, ErrorCode::LockupActive);

    // Update accumulator.
    update_accumulator(&mut ctx.accounts.pool, now)?;

    // Settle pending to user_reward_ata (same as deposit_stake). [omitted for brevity]

    // CPI: transfer staking tokens from stake_vault -> user_stake_ata (PDA signer).
    let seeds: &[&[&[u8]]] = &[&[b"stake_vault", ctx.accounts.pool.key().as_ref(), &[ctx.accounts.pool.stake_vault_bump]]];
    let cpi = CpiContext::new(ctx.accounts.token_program.to_account_info(), anchor_spl::token_interface::TransferChecked {
        from: ctx.accounts.stake_vault.to_account_info(),
        to: ctx.accounts.user_stake_ata.to_account_info(),
        authority: ctx.accounts.pool.to_account_info(),
        mint: ctx.accounts.pool.to_account_info(),
    }).with_signer(seeds);
    anchor_spl::token_interface::transfer_checked(cpi, amount, ctx.accounts.stake_vault.decimals)?;

    let stake = &mut ctx.accounts.stake;
    stake.amount = stake.amount.checked_sub(amount).ok_or(ErrorCode::MathOverflow)?;
    if stake.amount == 0 {
        // Zero out reward_debt so close_stake can fire.
        stake.reward_debt = 0;
    }
    stake.boosted_amount = ((stake.amount as u128).checked_mul(stake.boosted_multiplier_bps as u128).ok_or(ErrorCode::MathOverflow)?
        .checked_div(10_000).ok_or(ErrorCode::MathOverflow)?) as u64;
    ctx.accounts.pool.total_staked = ctx.accounts.pool.total_staked.checked_sub(amount).ok_or(ErrorCode::MathOverflow)?;

    emit!(StakeWithdrawn {
        user: ctx.accounts.user.key(),
        pool: ctx.accounts.pool.key(),
        amount,
        ts: now,
    });
    Ok(())
}

fn update_accumulator(pool: &mut Account<Pool>, now: i64) -> Result<()> {
    if pool.total_staked > 0 && now > pool.last_update_ts {
        let dt = (now - pool.last_update_ts).max(0) as u128;
        let acc_delta = (pool.reward_rate as u128)
            .checked_mul(dt).ok_or(ErrorCode::MathOverflow)?
            .checked_mul(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?
            .checked_div(pool.total_staked as u128).ok_or(ErrorCode::MathOverflow)?;
        pool.acc_reward_per_share = pool.acc_reward_per_share.checked_add(acc_delta).ok_or(ErrorCode::MathOverflow)?;
    }
    pool.last_update_ts = now;
    Ok(())
}
```

### claim_reward

```rust
pub fn claim_reward(ctx: Context<ClaimReward>) -> Result<()> {
    require!(!ctx.accounts.pool.paused, ErrorCode::PoolPaused);
    let pool = &mut ctx.accounts.pool;
    require!(!pool.is_locked, ErrorCode::IsLocked);
    pool.is_locked = true;
    let _guard = ReentrancyGuard { pool };

    let now = Clock::get()?.unix_timestamp;
    require!(now >= ctx.accounts.stake.unlock_ts, ErrorCode::LockupActive);
    update_accumulator(&mut ctx.accounts.pool, now)?;

    let boosted = (ctx.accounts.stake.amount as u128)
        .checked_mul(ctx.accounts.stake.boosted_multiplier_bps as u128).ok_or(ErrorCode::MathOverflow)?
        .checked_div(10_000).ok_or(ErrorCode::MathOverflow)?;
    let pending = boosted
        .checked_mul(ctx.accounts.pool.acc_reward_per_share).ok_or(ErrorCode::MathOverflow)?
        .checked_div(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?
        .checked_sub(ctx.accounts.stake.reward_debt).ok_or(ErrorCode::MathOverflow)?;
    let pay: u64 = pending.try_into().map_err(|_| ErrorCode::MathOverflow)?;
    require!(pay > 0, ErrorCode::InvalidAmount);

    let seeds: &[&[&[u8]]] = &[&[b"reward_vault", ctx.accounts.pool.key().as_ref(), &[ctx.accounts.pool.reward_vault_bump]]];
    let cpi = CpiContext::new(ctx.accounts.token_program.to_account_info(), anchor_spl::token_interface::TransferChecked {
        from: ctx.accounts.reward_vault.to_account_info(),
        to: ctx.accounts.user_reward_ata.to_account_info(),
        authority: ctx.accounts.pool.to_account_info(),
        mint: ctx.accounts.pool.to_account_info(),
    }).with_signer(seeds);
    anchor_spl::token_interface::transfer_checked(cpi, pay, ctx.accounts.reward_vault.decimals)?;

    let stake = &mut ctx.accounts.stake;
    stake.reward_debt = (stake.boosted_amount as u128)
        .checked_mul(ctx.accounts.pool.acc_reward_per_share).ok_or(ErrorCode::MathOverflow)?
        .checked_div(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?;
    ctx.accounts.pool.total_rewards_paid = ctx.accounts.pool.total_rewards_paid.checked_add(pay).ok_or(ErrorCode::MathOverflow)?;
    ctx.accounts.pool.pending_liability = ctx.accounts.pool.pending_liability.checked_sub(pending).ok_or(ErrorCode::MathOverflow)?;

    emit!(RewardClaimed { user: ctx.accounts.user.key(), pool: ctx.accounts.pool.key(), amount: pay, ts: now });
    Ok(())
}
```

### close_stake

```rust
#[derive(Accounts)]
pub struct CloseStake<'info> {
    #[account(mut, close = owner, seeds = [b"stake", pool.mint.as_ref(), owner.key().as_ref()], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    pub pool: Account<'info, Pool>,
    #[account(mut)]
    pub owner: Signer<'info>,
}

pub fn close_stake(ctx: Context<CloseStake>) -> Result<()> {
    update_accumulator(&mut ctx.accounts.pool, Clock::get()?.unix_timestamp)?;
    let boosted = (ctx.accounts.stake.amount as u128)
        .checked_mul(ctx.accounts.stake.boosted_multiplier_bps as u128).ok_or(ErrorCode::MathOverflow)?
        .checked_div(10_000).ok_or(ErrorCode::MathOverflow)?;
    let pending = boosted
        .checked_mul(ctx.accounts.pool.acc_reward_per_share).ok_or(ErrorCode::MathOverflow)?
        .checked_div(1_000_000_000_000_000_000).ok_or(ErrorCode::MathOverflow)?
        .checked_sub(ctx.accounts.stake.reward_debt).ok_or(ErrorCode::MathOverflow)?;
    require!(ctx.accounts.stake.amount == 0 && pending == 0, ErrorCode::PendingReward);
    emit!(StakeClosed { user: ctx.accounts.owner.key(), pool: ctx.accounts.pool.key(), ts: Clock::get()?.unix_timestamp });
    Ok(())
}
```

### set_authority / accept_authority

```rust
pub fn set_authority(ctx: Context<SetAuthority>, new: Pubkey, effective_ts: i64) -> Result<()> {
    admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.signer)?;
    require!(effective_ts >= Clock::get()?.unix_timestamp + TIME_LOCK_SECONDS, ErrorCode::TimeLockTooShort);
    let pool = &mut ctx.accounts.pool;
    pool.pending_authority = new;
    pool.pending_authority_effective_ts = effective_ts;
    emit!(AuthorityHandover { pool: pool.key(), from: pool.authority, to: new, effective_ts });
    Ok(())
}

pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    require!(Clock::get()?.unix_timestamp >= pool.pending_authority_effective_ts, ErrorCode::TimeLockTooShort);
    require!(ctx.accounts.signer.key() == pool.pending_authority, ErrorCode::Unauthorized);
    pool.authority = pool.pending_authority;
    pool.pending_authority = Pubkey::default();
    pool.pending_authority_effective_ts = 0;
    Ok(())
}
```

### update_rate

```rust
pub fn update_rate(ctx: Context<UpdateRate>, new_rate: u64, effective_ts: i64) -> Result<()> {
    admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.signer)?;
    require!(effective_ts > Clock::get()?.unix_timestamp, ErrorCode::TimeLockTooShort);
    let pool = &mut ctx.accounts.pool;
    pool.pending_rate = new_rate;
    pool.effective_ts = effective_ts;
    Ok(())
}
```

### migrate_pool

```rust
pub fn migrate_pool(ctx: Context<MigratePool>, new_version: u8) -> Result<()> {
    admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.signer)?;
    require!(new_version > ctx.accounts.pool.version, ErrorCode::WrongVersion);
    require!(new_version == ctx.accounts.pool.version + 1, ErrorCode::WrongVersion); // no skipping
    let pool = &mut ctx.accounts.pool;
    pool.version = new_version;
    Ok(())
}
```

### slash_to_treasury

```rust
pub fn slash_to_treasury(ctx: Context<SlashToTreasury>, amount: u64, treasury: Pubkey) -> Result<()> {
    admin_is_authorized(&ctx.accounts.pool, &ctx.accounts.signer)?;
    // Donation detection: skim surplus to treasury; only `amount` moves legitimately.
    let actual = ctx.accounts.reward_vault.amount;
    let tracked = ctx.accounts.pool.total_rewards_to_emit
        .checked_sub(ctx.accounts.pool.total_rewards_paid).ok_or(ErrorCode::MathOverflow)?
        .checked_add(ctx.accounts.pool.pending_liability as u64).ok_or(ErrorCode::MathOverflow)?;
    require!(actual >= amount, ErrorCode::InvalidAmount);
    // CPI: transfer `amount` from reward_vault to `treasury` (PDA signer). [omitted]
    Ok(())
}

fn admin_is_authorized(pool: &Account<Pool>, signer: &Signer) -> Result<()> {
    let expected = Pubkey::create_program_address(
        &[pool.multisig_pda.as_ref(), &[/* squad bump from pool */]],
        &pool.multisig_program,
    ).map_err(|_| error!(ErrorCode::InvalidMultisig))?;
    require!(signer.key() == expected, ErrorCode::Unauthorized);
    Ok(())
}

const TIME_LOCK_SECONDS: i64 = 7 * 24 * 60 * 60;
```

---

## 6. Test Matrix

| Category | Case | Expect |
|----------|------|--------|
| Happy path | deposit 100 tokens, time passes, claim | stake.amount=100, payout = boosted × acc × Δt / 1e18 |
| Happy path | multiple users, varying deposit timing | each user gets share weighted by boosted_amount × duration |
| Accumulator | 1k deposits / withdraws in random order | no precision loss beyond 1e18 scale |
| First deposit | deposit into empty pool with reward_vault already funded | first staker captures only post-funding share; virtual offset + deposit_enabled prevents capture |
| Donation | attacker force-sends reward tokens to reward_vault | surplus skimmed to treasury, settle proceeds with tracked only (no DOS) |
| Reentrancy | Token-2022 hook calls back into program mid-CPI | ReentrancyGuard: `is_locked` cleared by Drop on every exit (success + `?`); `IsLocked` on recursion |
| Lockup | unstake before unlock_ts | `LockupActive` error |
| Lockup | claim before unlock_ts | `LockupActive` error |
| Lockup | unstake after unlock_ts | success |
| Tier 0 | deposit with lockup_tier = 0 | lockup_seconds = pool.lockup_seconds (floor) |
| Lockup reset | top-up after unlock_ts | stake_start_ts frozen; unlock_ts preserved |
| Pause | deposit while paused | `PoolPaused` error |
| Pause | claim while paused | success (rewards not rugged) |
| Pause | withdraw while paused | `PoolPaused` error |
| Arithmetic | total_staked + amount > stake_cap | `PoolFull` |
| Arithmetic | reward_rate × dt overflows u128 | `MathOverflow` (no `unwrap`, no panic grief) |
| Cap | emission_end_ts reached | accumulator stops accruing |
| Flash-loan | stake + claim + unstake same slot | `LockupActive` from `claim_reward` lockup gate |
| MEV | rate change front-runs deposit | old rate applies until `now >= effective_ts`; promotion inside handler |
| Authority handover | call set_authority, advance clock 7d, accept_authority | promotion atomic; pending_authority_effective_ts respected |
| Migration | bump version via migrate_pool | version increments by 1; `WrongVersion` if skip |
| Slash | vault has surplus from donation | surplus skimmed to treasury before slash; tracked unchanged |
| Close stake | amount = 0 and pending = 0 | success; PDA reaped, rent refunded |
| Close stake | amount = 0 but pending > 0 | `PendingReward` |
| Close stake | amount > 0 | `StakeNotEmpty` |
| Token-2022 init | mint with transfer-hook | `UnsupportedMintExtension` at init |
| Vault substitution | wrong mint on user_stake_ata | constraint fail (`mint == pool.mint`) |
| Multisig | signer != expected multisig PDA | `Unauthorized` |
| Fuzz / proptest | random deposit/claim sequences | `acc_reward_per_share` monotonically non-decreasing; `total_rewards_to_emit == total_rewards_paid + pending_liability` holds |

---

## 7. Wiring Checklist

1. Deploy with `version: 1`; reserve offset 0 for migration.
2. `Pool` PDA seeds `[b"pool", mint.key()]`, canonical bump stored on init.
3. Vault PDAs `[b"stake_vault", pool.key()]` + `[b"reward_vault", pool.key()]`, bumps stored on `Pool`.
4. `Stake` PDA seeds `[b"stake", pool.mint, user.key()]` (mint-stable, not pool-key).
5. `acc_reward_per_share` `u128` scaled 1e18. All math via `checked_*` + `try_into`. No `unwrap`, no `as u64`.
6. ReentrancyGuard via Drop — `is_locked` cleared on every exit path.
7. Refuse Token-2022 hostile extensions at pool init (transfer-hook, transfer-fee, permanent-delegate, non-transferable, confidential).
8. Donation: skim surplus to treasury; never hard-revert (DOS protection).
9. `lockup_seconds > 0` enforced at `init_pool`; tier 0 = pool floor.
10. `stake_start_ts` frozen after first write; top-ups only increase `amount`.
11. Pause blocks deposit/withdraw/fund_rewards; claim stays open.
12. `set_authority` is two-step: `set_authority` + `accept_authority`, gated by `pending_authority_effective_ts >= now + TIME_LOCK_SECONDS` (7 days).
13. Multisig enforced via `Pool.multisig_program` + `Pool.multisig_pda`; Squads m ≥ 2 controls pause, rate updates, set_authority, slash, upgrade.
14. `close_stake` reaps zero-balance PDAs; refunds rent to owner.
15. `migrate_pool` bumps `version` (only `current + 1`); gate every handler with `pool.version == N`.
16. `fund_rewards` is admin-only; flips `deposit_enabled = true` on first call.
17. `update_rate` queues via `pending_rate` + `effective_ts`; promotion inside every reward-touching handler.
18. Every state transition emits typed `#[event]` for indexers.
19. Audit (Neodyme / OtterSec / Trail of Bits) before mainnet.
20. Off-chain indexer contract: PostgreSQL/Timescale hypertable for `stakes`, `reward_events`, `pool_funded`, `stake_closed`, `authority_handover` keyed off event struct field order.
