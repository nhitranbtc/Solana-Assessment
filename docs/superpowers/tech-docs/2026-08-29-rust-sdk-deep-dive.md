# Rust SDK + Crate Deep Dive — Meme Coin Build

> Research dossier for building `Solana-Assessment` per the 7 superpowers plans.
> Scope: every Rust crate referenced, every SDK API surface touched, every CPI pattern used.
> Toolchain pin: Rust 1.79.0, Solana 1.18.26, Anchor 0.30.1, Node 20+.

## Status snapshot

- **Workspace plan** (15 tasks): Tasks 1-9 complete (toolchain, manifests, `declare_id!`, errors, events, state namespace). Tasks 10-15 remaining (hostile-extension guard, CI, indexer doc, legacy deletion).
- **Module plans** (6 × 4-7 tasks): none started.
- **Test plan**: integration-test crate scaffolded (`tests/Cargo.toml` + empty `src/lib.rs`).
- **Legacy top-level stubs** (`/lib.rs`, `/token.rs`, `/airdrop.rs`, `/presale.rs`, `/dev_fund.rs`, `/liquidity.rs`): present, buggy, NOT wired into Anchor program. Will be deleted in workspace Task 15.

`★ Insight ─────────────────────────────────────`
Every module plan assumes workspace Tasks 10-11 (hostile-extension guard) land first. The `common::assert_no_hostile_extensions` symbol is referenced in every module's instruction handler. Workspace must finish before any module plan starts.
`─────────────────────────────────────────────────`

---

## 1. Crate stack — exact versions

### 1.1 Solana runtime

| Crate | Version | Role |
|---|---|---|
| `solana-program` | `1.18.26` | `Pubkey`, `AccountInfo`, `Clock`, sysvars, keccak, `invoke`, `invoke_signed`, `system_program::transfer`. Program-side base. |
| `solana-sdk` | `1.18.26` | Client-side: `Keypair`, `Signer`, `Instruction`, `Transaction`. Test harness. |
| `solana-program-test` | `1.18.26` | `ProgramTest` in-process validator. Boots every integration test. |
| `solana-client` | `1.18.26` (optional) | RPC client for off-chain indexer/bots. Not used in plans. |

### 1.2 Anchor

| Crate | Version | Features | Role |
|---|---|---|---|
| `anchor-lang` | `0.30.1` | `init-if-needed`, `idl-build`, `cpi`, `no-entrypoint`, `no-idl`, `no-log-ix-name` | Program macros (`#[program]`, `#[account]`, `#[derive(Accounts)]`), `Context<T>`, `CpiContext`, `AnchorSerialize/Deserialize`. **Critical**: `idl-build` feature (enabled in workspace Cargo.toml) is what emits `target/idl/meme_coin.json` + generates `meme_coin::instruction::{InitializeToken, InitializeAirdrop, ...}::data()` builders used by every integration test. |
| `anchor-spl` | `0.30.1` | `init-if-needed` | Token + ATA helpers: `token::Mint`, `token::TokenAccount`, `associated_token::AssociatedToken`, `token::transfer`, `token::mint_to`, `token::set_authority`, `accessor::amount`. |

### 1.3 SPL (Solana Program Library)

| Crate | Version | Role |
|---|---|---|
| `spl-token` | `3.0` (workspace pin); `4.x` referenced in plan bodies | `spl_token::state::Mint::LEN`, `spl_token::instruction::{initialize_mint, mint_to, transfer}`, `AuthorityType::{MintTokens, FreezeAccount}`. |
| `spl-token-2022` | `3.0` (workspace pin); `4.x` referenced in plan bodies | `extension::BaseStateWithExtensions`, `extension::ExtensionType`, `state::Mint`, `instruction::create_mint_with_transfer_fee`. `no-entrypoint` feature enabled. |
| `spl-associated-token-account` | `3.0` | `get_associated_token_address`, `get_associated_token_address_with_program_id`, `instruction::create_associated_token_account`. |

### 1.4 Metaplex (token plan, Task 6)

| Crate | Version | Role |
|---|---|---|
| `mpl-token-metadata` | `4.0` | Token Metadata program client. CPI to `CreateMetadataAccountV3`. |

Imports:
```rust
use mpl_token_metadata::instructions::{
    CreateMetadataAccountV3, CreateMetadataAccountV3Instruction, CreateMetadataAccountV3InstructionArgs,
};
use mpl_token_metadata::types::DataV2;
use mpl_token_metadata::ID;
```
Program id: `metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s`.

### 1.5 Raydium CPMM (liquidity plan, Task 3)

| Crate | Version | Role |
|---|---|---|
| `raydium-cpmm-cpi` | `0.4` | Raydium CPMM CPI client. `initialize_pool2` + `deposit`. |

Program id pinned in `liquidity/constants.rs` (must match `solana_program::pubkey!` macro at compile time — verify against `raydium-cpmm-cpi` SDK's published constants at integration):
```rust
pub const RAYDIUM_CPMM_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("CPMMoo8L3F4NbTegBCKVNungg7vB3BUxpgd6QAj9Ck7");
```

### 1.6 Test infrastructure

| Crate | Role |
|---|---|
| `tokio` | Async runner (`#[tokio::test]`). |
| `anchor-lang::prelude::*` | Common imports in tests. |
| `solana_program_test::*` | `ProgramTest`, `processor!` macro. |
| `anchor_lang::AccountDeserialize` | Decode on-chain accounts in assertions. |

### 1.7 Tooling

| Tool | Version | Role |
|---|---|---|
| `rustc` | `1.79.0` | Compiler (pinned via `rust-toolchain.toml`). |
| `solana-cli` | `1.18.26` | `solana-keygen`, `anchor deploy` underneath. |
| `anchor-cli` | `0.30.1` | `anchor build` → IDL + BPF; `anchor test` → ts-mocha; `anchor deploy`. |
| `node` | `>=20.0.0` | Anchor TS client + IDL codegen. |
| `avm` | latest | Anchor Version Manager. |
| `cargo install --git https://github.com/coral-xyz/anchor --tag v0.30.1 --locked` | CI bootstrap. |

`★ Insight ─────────────────────────────────────`
The `solana-program-test` bakes the runtime version. Mismatched `solana-sdk` + `solana-program-test` panics on startup. CI installs Solana CLI `v1.18.26` to match. The Cargo.toml pins `3.0` for SPL crates while plan code samples reference `4.x` — known drift. Pin to `3.0` until toolchain bump.
`─────────────────────────────────────────────────`

---

## 2. Rust SDK mechanics

### 2.1 PDA seed conventions (from all plans)

```rust
// programs/meme-coin/src/token/constants.rs
pub const MINT_AUTHORITY_SEED: &[u8] = b"mint_authority";
pub const METADATA_AUTHORITY_SEED: &[u8] = b"metadata_authority";
pub const TOKEN_CONFIG_SEED: &[u8] = b"token_config";

// programs/meme-coin/src/airdrop/constants.rs
pub const AIRDROP_SEED: &[u8] = b"airdrop";
pub const CLAIM_SEED: &[u8] = b"claim";
pub const VAULT_SEED: &[u8] = b"airdrop_vault";

// programs/meme-coin/src/presale/constants.rs
pub const PRESALE_SEED: &[u8] = b"presale";
pub const SOL_VAULT_SEED: &[u8] = b"presale_sol_vault";
pub const MINT_AUTHORITY_SEED: &[u8] = b"presale_mint_authority";  // collides with token
pub const CONTRIBUTION_SEED: &[u8] = b"contribution";

// programs/meme-coin/src/vesting/constants.rs
pub const VESTING_SEED: &[u8] = b"vesting";
pub const VAULT_SEED: &[u8] = b"vesting_vault";
pub const MAX_RELEASE_POINTS: usize = 256;

// programs/meme-coin/src/liquidity/constants.rs
pub const POOL_AUTHORITY_SEED: &[u8] = b"pool_authority";
pub const TOKEN_VAULT_SEED: &[u8] = b"pool_token_vault";
pub const SOL_VAULT_SEED: &[u8] = b"pool_sol_vault";
pub const POOL_STATE_SEED: &[u8] = b"pool_state";

// programs/meme-coin/src/staking/constants.rs
pub const POOL_SEED: &[u8] = b"staking_pool";
pub const STAKE_VAULT_SEED: &[u8] = b"staking_stake_vault";
pub const REWARD_VAULT_SEED: &[u8] = b"staking_reward_vault";
pub const STAKE_ENTRY_SEED: &[u8] = b"stake_entry";
pub const ONE: u128 = 1_000_000_000_000_000_000;  // 1e18 fixed-point
```

Two-tier pattern: `state_pda = [module_seed, mint_or_beneficiary_key]`, `vault_authority = [vault_seed, state_pda_key]`. Bumps stored on state, re-derived via `find_program_address` in tests, used via `invoke_signed` in handlers.

`★ Insight ─────────────────────────────────────`
`presale::constants::MINT_AUTHORITY_SEED` collides in name with `token::constants::MINT_AUTHORITY_SEED`. Rust modules namespace via `crate::presale::constants::MINT_AUTHORITY_SEED`, so no compile conflict. Consider renaming `presale_mint_authority` for grep clarity.
`─────────────────────────────────────────────────`

### 2.2 Anchor account constraint patterns

| Constraint | Used by | Purpose |
|---|---|---|
| `#[account(init, payer = X, space = N, seeds = [...], bump)]` | every state account | Create + rent + PDA verify |
| `#[account(init_if_needed, payer = X, space = N, seeds = [...], bump)]` | presale `Contribution`, staking `StakeEntry` | Idempotent re-use. **Race window**: Anchor skips init if account exists with correct discriminator. Client A + Client B same slot = both see "exists" = both proceed against initialized state without re-init. Safe ONLY when subsequent ix logic correctly handles re-entry (e.g., `Contribution.tokens_bought` uses `checked_add`). Plan presale + staking both handle this; review any future use. |
| `#[account(mut, seeds = [...], bump = stored_bump)]` | every PDA vault on mutations | Canonical bump |
| `#[account(constraint = account.mint == expected @ ErrorCode::MintMismatch)]` | presale `buyer_token_account`, vesting `beneficiary_token_account` | Cross-account check |
| `#[account(has_one = authority @ ErrorCode::Unauthorized)]` | presale `FinalizePresale`, vesting `RevokeVesting` | Single-source auth |
| `#[account(init, payer, space, token::mint = X, token::authority = Y)]` | airdrop `vault`, vesting `vault`, staking vaults | Auto-create token account |
| `#[account(init, payer, associated_token::mint = X, associated_token::authority = Y)]` | token `treasury_token_account` | Auto-create ATA |
| `/// CHECK: ...` | every raw `AccountInfo` (PDA signers, Metaplex, treasury wallets) | Suppresses Anchor's auto-safety |

### 2.3 CPI patterns

| Call | Used by |
|---|---|
| `anchor_spl::token::transfer(CpiContext::new(...), amount)` | airdrop fund + claim, staking fund + stake + withdraw + claim, liquidity transfer |
| `anchor_spl::token::mint_to(CpiContext::new_with_signer(...), amount)` | token initialize (PDA mint auth), presale buy |
| `anchor_spl::token::set_authority(CpiContext::new_with_signer(...), AuthorityType, None)` | token initialize — revoke mint + freeze |
| `anchor_lang::system_program::transfer(CpiContext::new(...), lamports)` | presale buy (buyer → SOL vault) |
| Direct lamport mutation `**from.try_borrow_mut_lamports()? -= N; **to.try_borrow_mut_lamports()? += N` | presale finalize, liquidity SOL deposit (PDA → user) |
| `anchor_spl::token::accessor::amount(&account_info)?` | liquidity LP burn, staking donation guard |
| `CreateMetadataAccountV3Instruction { ... }.invoke_signed(&[...])` | token initialize (Metaplex) |
| `raydium_cpmm_cpi::cpi::initialize_pool2(...)?` + `deposit(...)?` | liquidity initialize (gated on Task 3) |

`★ Insight ─────────────────────────────────────`
Direct lamport mutation is **only safe** when both source and destination are PDA-owned by your program. CPI to `system_program::transfer` preferred when source is a user wallet (saves you from signing). Presale finalize uses direct mutation (PDA→treasury, both PDA); presale buy uses system CPI (user→PDA).
`─────────────────────────────────────────────────`

### 2.4 Error & event integration

- **Single `ErrorCode` enum** in `programs/meme-coin/src/errors.rs`. Re-exported at crate root via `pub use errors::ErrorCode;`. Every `require!` / `Err(error!(ErrorCode::X))` references the same enum.
- **Single `events.rs`** for all `#[event]` types. Every state-mutating ix emits exactly one event per workspace plan. `#[index]` on `Pubkey` discriminator fields for indexer filtering.

---

## 3. Module deep dives

### 3.1 Token (`programs/meme-coin/src/token/`)

**Anchor SDK calls** (Tasks 3 + 6 of token plan):

```rust
use anchor_spl::token::{self, SetAuthority};
use spl_token::instruction::AuthorityType;
use mpl_token_metadata::instructions::{
    CreateMetadataAccountV3, CreateMetadataAccountV3Instruction, CreateMetadataAccountV3InstructionArgs,
};
use mpl_token_metadata::types::DataV2;
```

**InitializeToken accounts struct** (Task 6):
```rust
#[derive(Accounts)]
pub struct InitializeToken<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(init, payer = payer, space = TokenConfig::LEN,
               seeds = [TOKEN_CONFIG_SEED, mint.key().as_ref()], bump)]
    pub token_config: Account<'info, TokenConfig>,
    #[account(init, payer = payer, mint::decimals = 9, mint::authority = mint_authority)]
    pub mint: Account<'info, anchor_spl::token::Mint>,
    /// CHECK: PDA seeds = [MINT_AUTHORITY_SEED, mint.key()]
    #[account(seeds = [MINT_AUTHORITY_SEED, mint.key().as_ref()], bump)]
    pub mint_authority: AccountInfo<'info>,
    /// CHECK: PDA seeds = [METADATA_AUTHORITY_SEED, mint.key()]
    #[account(seeds = [METADATA_AUTHORITY_SEED, mint.key().as_ref()], bump)]
    pub metadata_authority: AccountInfo<'info>,
    /// CHECK: Metaplex metadata PDA, created by Metaplex CPI
    #[account(mut)] pub metadata: AccountInfo<'info>,
    #[account(init, payer = payer, associated_token::mint = mint, associated_token::authority = treasury)]
    pub treasury_token_account: Account<'info, anchor_spl::token::TokenAccount>,
    /// CHECK: treasury wallet
    pub treasury: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, anchor_spl::token::Token>,
    pub associated_token_program: Program<'info, anchor_spl::associated_token::AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}
```

**Handler flow**:
1. Validate `total_supply != 0` + not already initialized.
2. `common::assert_no_hostile_extensions(&InterfaceAccount::try_from(mint)?)?` — pre-flight guard.
3. Compute `mint_authority_bump = ctx.bumps.mint_authority`.
4. `token::mint_to(CpiContext::new_with_signer(..., &[mint_authority_seeds]), total_supply)?` — PDA mints to treasury ATA.
5. `token::set_authority(..., AuthorityType::MintTokens, None)?` — revoke mint.
6. `token::set_authority(..., AuthorityType::FreezeAccount, None)?` — revoke freeze.
7. CPI Metaplex `CreateMetadataAccountV3Instruction { ... }.invoke_signed(&[mint_authority_seeds, metadata_authority_seeds])?` with `is_mutable: false`. **Combined-seeds requirement**: Metaplex's `CreateMetadataAccountV3` expects both `mint_authority` (signer for mint) + `update_authority` (signer for metadata). Pass BOTH seed slices in the `invoke_signed` array; single-PDA signing fails.
8. Persist state, emit `TokenInitialized { mint, decimals, total_supply, metadata_pda }`.

`★ Insight ─────────────────────────────────────`
`#[account(init, ..., mint::decimals = 9, mint::authority = mint_authority)]` is Anchor 0.30 sugar. Equivalent to manual `system_instruction::create_account` + `spl_token::instruction::initialize_mint` CPI, with rent + signer validation baked in.
`─────────────────────────────────────────────────`

### 3.2 Airdrop (`programs/meme-coin/src/airdrop/`)

**Merkle hashing**: `solana_program::keccak::hashv` — cheaper than SHA256 + tree-friendly with sorted-pair.

**Pure-Rust helper** (Task 2):
```rust
use solana_program::keccak::hashv;

pub fn verify_proof(root: [u8; 32], leaf: [u8; 32], proof: &[[u8; 32]]) -> bool {
    let mut computed = leaf;
    for sibling in proof {
        // Pair-wise sorted hash: smaller-hash-first prevents second-preimage attacks.
        let (a, b) = if computed <= *sibling { (computed, *sibling) } else { (*sibling, computed) };
        let combined: Vec<u8> = a.iter().chain(b.iter()).copied().collect();
        computed = hashv(&[&combined]).to_bytes();
    }
    computed == root
}

pub fn leaf_hash(user: &[u8; 32], amount: u64) -> [u8; 32] {
    let amount_bytes = amount.to_le_bytes();
    let combined: Vec<u8> = user.iter().chain(amount_bytes.iter()).copied().collect();
    hashv(&[&combined]).to_bytes()
}
```

**Anti-double-claim**: per-user `Claim` PDA = `init` constraint on `claim_account`. Second `claim_airdrop` for same user reverts because Anchor tries to re-init existing account.

### 3.3 Presale (`programs/meme-coin/src/presale/`)

**Tier pricing — pure Rust, no floats**:
```rust
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TierPrice { pub cap_sold: u64, pub price_lamports_per_token: u64 }

pub fn price_for_next_token(tiers: &[TierPrice], total_sold: u64) -> Result<u64> {
    for tier in tiers {
        if total_sold < tier.cap_sold { return Ok(tier.price_lamports_per_token); }
    }
    Err(error!(ErrorCode::HardCapExceeded))
}
```

**SOL handling**:
- buyer → SOL vault: `system_program::transfer(CpiContext::new(...), lamports)` — user signs.
- vault → treasury on finalize: direct lamport mutation — both PDA.

**Slippage guard**: `total_cost > expected_total_lamports` → revert. Buyer passes max acceptable cost.

**Refund path**: only enabled when `!presale.reached_soft_cap`. Reads stored `lamports_paid` from per-buyer `Contribution` PDA (snapshot at finalize time).

### 3.4 Vesting (`programs/meme-coin/src/vesting/`)

**Stored schedule** (not clock math at init):
```rust
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ReleasePoint { pub ts: i64, pub amount: u64 }
pub schedule: Vec<ReleasePoint>  // capped at MAX_RELEASE_POINTS = 256
```

**Release math**: `releasable_now = sum(p.amount for p in schedule if p.ts <= now)`, `amount_to_release = releasable_now - total_released`. Transfer if > 0.

`★ Insight ─────────────────────────────────────`
Stored-schedule vs. linear math: stored = rigid but auditable + verifiable on-chain. Cost = 256 points × 16 bytes = 4 KiB per vesting account. Hybrid option: store checkpoints + linear interpolation between.
`─────────────────────────────────────────────────`

### 3.5 Liquidity (`programs/meme-coin/src/liquidity/`)

**Raydium CPMM CPI** (Task 3, gated on SDK pin):
```rust
raydium_cpmm_cpi::cpi::initialize_pool2(...)?;
raydium_cpmm_cpi::cpi::deposit(...)?;
```

**API drift risk**: `raydium-cpmm-cpi 0.4` may rename `initialize_pool2` → `initialize` (newer SDK convention). At integration time, verify against `raydium-cpmm-cpi` crate's published instruction enum. If renamed, swap call site + re-run liquidity integration test against Raydium CPMM devnet pool.

**LP burn** (Task 2): after Raydium mints LP to `pool_lp_token_account`, immediately transfer full balance to `lp_burn_destination` (known burn address). Irreversible proof of locked liquidity.

`★ Insight ─────────────────────────────────────`
Direct lamport mutation for SOL → Raydium SOL vault bypasses system_program because source is a PDA owned by your program. Cleaner alternative = `system_program::transfer` with PDA signer seeds; both work.
`─────────────────────────────────────────────────`

### 3.6 Staking (`programs/meme-coin/src/staking/`)

**Math** (MasterChef v2 pattern, u128 / 1e18):
```rust
pub const ONE: u128 = 1_000_000_000_000_000_000;

pub fn settle_pending_rewards(pool: &mut PoolState, now: i64) -> Result<()> {
    if now <= pool.last_update_ts || pool.total_staked == 0 {
        pool.last_update_ts = now;
        return Ok(());
    }
    let dt: u128 = (now as u128).checked_sub(pool.last_update_ts as u128)?;
    let emission: u128 = dt.checked_mul(pool.reward_rate_per_sec as u128)?;
    let acc_increment: u128 = emission.checked_mul(ONE)?.checked_div(pool.total_staked as u128)?;
    pool.acc_reward_per_share = pool.acc_reward_per_share.checked_add(acc_increment)?;
    pool.last_update_ts = now;
    Ok(())
}

pub fn pending_reward(stake: &StakeEntry, pool: &PoolState) -> Result<u64> {
    let accrued: u128 = (stake.amount as u128)
        .checked_mul(pool.acc_reward_per_share)?
        .checked_div(ONE)?;
    let owed: u128 = accrued.checked_sub(stake.reward_debt)?;
    u64::try_from(owed).map_err(|_| error!(ErrorCode::Overflow))
}
```

**Donation guard** in `fund_rewards`:
```rust
let actual = anchor_spl::token::accessor::amount(&reward_vault.to_account_info())?;
let expected = pool.last_tracked_total.checked_add(amount)?;
if actual > expected {
    let surplus = actual.checked_sub(expected)?;
    // transfer surplus back to funder (skims, never reverts)
}
pool.last_tracked_total = expected;
```

**Lockup semantics**: `now >= stake_entry.unlock_ts` required for both `withdraw_stake` AND `claim_reward`. Top-ups preserve original `unlock_ts`.

**First-deposit virtual offset**: `acc_reward_per_share = ONE` at init — prevents first-staker-steals-share attack when emission lands between init and first deposit.

---

## 4. Test harness patterns

### 4.1 ProgramTest setup

Every integration test starts:
```rust
let pt = ProgramTest::new("meme_coin", meme_coin::id(), processor!(meme_coin::entry));
pt.add_program("spl_token_2022", spl_token_2022::id(), None);  // only for T22 tests
let (banks_client, payer, recent) = pt.start().await;
```

`processor!(meme_coin::entry)` requires the workspace plan's `entry` shim (Task 11):
```rust
#[no_mangle]
pub fn entry(program_id: &Pubkey, accounts: &[AccountInfo], ix_data: &[u8]) -> ProgramResult {
    anchor_lang::entrypoint::entry(program_id, accounts, ix_data)
}
```

### 4.2 Anchor instruction data builders

`anchor build` emits `meme_coin::instruction::{InitializeToken, InitializeAirdrop, ...}` structs. Each has `.data() -> Vec<u8>`:
```rust
let ix_data = meme_coin::instruction::InitializePresale {
    start_ts, end_ts, soft_cap_lamports, hard_cap_lamports, tiers,
}.data();

let ix = Instruction {
    program_id: meme_coin::id(),
    accounts: vec![/* AccountMeta list in plan-declared order */],
    data: ix_data,
};
```

`★ Insight ─────────────────────────────────────`
Account order in `accounts: vec![...]` must match the `#[derive(Accounts)]` struct field order exactly. Mismatch = Anchor runtime rejects with "missing required account" or "unknown account". Tests always read fields in declared order.
`─────────────────────────────────────────────────`

### 4.3 PDA derivation in tests

```rust
let (presale_pda, bump) = Pubkey::find_program_address(
    &[crate::presale::constants::PRESALE_SEED, mint.pubkey().as_ref()],
    &meme_coin::id(),
);
```

`Pubkey::find_program_address` returns canonical bump. For fixed bumps: `Pubkey::create_program_address` (returns `Option`).

### 4.4 Account state assertions

```rust
let account = banks_client.get_account(pda).await.unwrap().unwrap();
let state: PresaleState =
    anchor_lang::AccountDeserialize::try_deserialize(&mut &account.data[..]).unwrap();
assert_eq!(state.total_sold, 100);
```

`AccountDeserialize` deserializes 8-byte discriminator + Borsh payload. Fails fast on mismatch.

---

## 5. Cross-cutting Rust patterns

| Pattern | SDK call | Use case |
|---|---|---|
| Signer check | `pub authority: Signer<'info>` | All admin ix require explicit Signer |
| PDA signer | `AccountInfo` + `#[account(seeds = ..., bump)]` + `invoke_signed` | Program signs on behalf of PDA-owned vault |
| PDA bump canonicalization | `bump = ctx.bumps.vault_authority` | Store bump from Anchor's computation |
| Lamport transfer (user) | `system_program::transfer(CpiContext::new(...), lamports)` | User → PDA vault |
| Lamport transfer (PDA) | `**from.lamports.borrow_mut() -= N; **to.lamports.borrow_mut() += N` | PDA → user (no signer needed) |
| Token transfer | `anchor_spl::token::transfer(CpiContext, amount)` | Universal |
| Token mint | `anchor_spl::token::mint_to(CpiContext::new_with_signer(...), amount)` | PDA-signed mint |
| Set authority | `anchor_spl::token::set_authority(CpiContext, AuthorityType, new_authority: Option<Pubkey>)` | Revoke = `None` |
| Time check | `Clock::get()?.unix_timestamp` | All time windows |
| Rent | `banks_client.get_rent().await.unwrap().minimum_balance(LEN)` | For raw `system_instruction::create_account` in tests |
| Token-2022 extension check | `Mint::try_deserialize(...).get_extension_types()` → iterate vs `HOSTILE_EXTENSIONS` | Gate every entry touching a mint |
| Account close | `#[account(close = destination)]` | Refund lamports + zero data on close |
| Account resize | `#[account(realloc = N, realloc::payer = X, realloc::zero = false)]` | Resize for growing data |
| Error logging | `msg!("context: {}", value); err!(ErrorCode::X)` | Debug + structured errors |
| Event emission | `emit!(Event { ... })?` | Indexed for indexers |
| Read token balance | `anchor_spl::token::accessor::amount(&account_info)?` → `Result<u64>` | Decoded `TokenAccount.amount` field. Used by liquidity LP burn (post-CPI balance read) + staking donation guard (`vault.amount > expected` check). Bypasses manual `TokenAccount::try_deserialize`. |

---

## 6. Build order (synthesized)

1. **Workspace plan** (15 tasks): toolchain → manifest → errors → events → common guard → CI → indexer doc → legacy deletion. **Tasks 1-9 done; 10-15 remaining.**
2. **Token** (7 tasks): state scaffold → accounts → impl → hostile guard test → Metaplex → clippy. **Highest priority — every other plan references its mint.**
3. **Airdrop** (6 tasks): state → merkle helper → impl + handlers → scaffold test → green e2e → clippy.
4. **Vesting** (4 tasks): state → handlers → integration → clippy.
5. **Presale** (5 tasks): state → tier pricing helper → handlers → integration → clippy.
6. **Liquidity** (5 tasks): state → handler scaffold → Raydium SDK wiring → test fixture → clippy. **Task 3 gated on SDK pin.**
7. **Staking** (5 tasks): state → math helpers → handlers → test fixture → clippy.

**Total: 47 tasks across all 7 plans** (workspace 15 + token 7 + airdrop 6 + vesting 4 + presale 5 + liquidity 5 + staking 5).

Recommended execution: **subagent-driven** (one fresh agent per task, review between tasks). Inline execution possible but bottlenecks on review.

---

## 7. Crate-by-crate import cheat sheet

```rust
// =================== Token-2022 hostile extensions ===================
use spl_token_2022::extension::ExtensionType;
use spl_token_2022::state::Mint;
// Used by common.rs (workspace Task 11)
// Get extensions: Mint::try_deserialize(&mut &data[..]).ok().map(|m| m.get_extension_types())

// =================== SPL Token ===================
use spl_token::instruction::{AuthorityType, initialize_mint, mint_to, transfer};
use spl_token::state::{Mint, Account as TokenAccount};
// AuthorityType variants: MintTokens, FreezeAccount, AccountOwner, CloseAccount

// =================== Associated Token Account ===================
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
    instruction as ata_ix,
};

// =================== anchor_spl sugar ===================
use anchor_spl::token::{self, Mint, Token, TokenAccount, SetAuthority};
use anchor_spl::associated_token::AssociatedToken;
// token::transfer(cpi_ctx, amount)? — direct transfer
// token::mint_to(CpiContext::new_with_signer(...), amount)? — mint
// token::set_authority(CpiContext::new_with_signer(...), AuthorityType::MintTokens, None)? — revoke

// =================== Anchor ===================
use anchor_lang::prelude::*;
use anchor_lang::system_program;

// =================== Solana SDK ===================
use solana_program::pubkey::Pubkey;
use solana_program::keccak::hashv;
use solana_program::instruction::Instruction;
use solana_program_test::{ProgramTest, processor};
use solana_sdk::{
    signature::Keypair, signer::Signer,
    transaction::Transaction, system_instruction,
    instruction::AccountMeta, sysvar::rent,
};

// =================== Metaplex ===================
use mpl_token_metadata::instructions::{
    CreateMetadataAccountV3, CreateMetadataAccountV3Instruction, CreateMetadataAccountV3InstructionArgs,
};
use mpl_token_metadata::types::DataV2;
use mpl_token_metadata::ID;  // = metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s

// =================== Raydium CPMM ===================
use raydium_cpmm_cpi;  // see SDK docs for actual call surface
// Constants::RAYDIUM_CPMM_PROGRAM_ID = "CPMMoo8L3F4NbTegBCKVNungg7vB3BUxpgd6QAj9Ck7"
```

---

## 8. Common pitfalls (from plan self-reviews + code review)

| Pitfall | Where | Fix |
|---|---|---|
| `f64` for money | root stub `presale.rs` | Integer math only (u64 / u128) |
| Wrong month-second constant | root stub `dev_fund.rs` (`24*3600*365` ≈ year, not month) | `30 * 86_400` for month |
| `lamports()` balance check vs actual payment intent | root stub `presale.rs` | `system_program::transfer` CPI |
| Client-supplied PDA bump | anti-pattern | Store bump on-chain, use `bump = stored_bump` |
| Float division | tier pricing | Integer comparisons (`< cap_sold`) |
| `init` vs `init_if_needed` race | contribution + stake entry | Document + review each use |
| Single-enum `ErrorCode` discipline | every module | Add variants to existing enum; never spawn parallel |
| `#[index]` on event fields | every `#[event]` | Mark all Pubkey/mint/authority fields |
| Token-2022 hostile extensions slipping past | every mint-touching ix | `common::assert_no_hostile_extensions` first |
| Reentrancy via CPI | airdrop claim, liquidity swap | Update state before CPI (Anchor serializes) |
| Donation to reward vault | staking | `last_tracked_total` + skim excess |
| First-staker attack | staking | `acc_reward_per_share = ONE` at init |

---

## 9. Staking math — the only pure-Rust unit test

Across all 7 plans, the staking `settle_pending_rewards` test is the **only** pure-Rust unit test. Every other test boots `ProgramTest`. Verifying this math passes after helpers land = the first sanity check that the math primitives are sound.

Expected output:
```
acc_reward_per_share = ONE * 2
last_update_ts = 200
pending_reward = 100
```

Math breakdown:
- `dt = 200 - 100 = 100` seconds
- `emission = 100 * 10 = 1000`
- `acc_increment = 1000 * 1e18 / 1000 = 1e18`
- new `acc_reward_per_share = 1e18 + 1e18 = 2e18`
- `pending = 100 * 2e18 / 1e18 - 100 * 1e18 / 1e18 = 200 - 100 = 100`

---

## 10. Verification commands

```bash
# Workspace build
cd /home/nhitran/Projects/Solana-Assessment
anchor build
# expected: target/deploy/meme_coin.so + target/idl/meme_coin.json

# Run all tests
anchor test --skip-deploy
# expected: every test in 7 plans passes

# Lint
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Verify IDL contains all ix handlers
grep -c "InitializeToken\|InitializeAirdrop\|InitializePresale\|InitializeVesting\|InitializePool\|InitStakingPool\|ClaimAirdrop\|BuyTokens\|FinalizePresale\|ClaimRefund\|InitializeAirdrop\|ReleaseVested\|RevokeVesting\|Stake\|WithdrawStake\|ClaimReward\|EmergencyWithdraw\|FundRewards" \
  target/idl/meme_coin.json
# expected: >= 18 matches

# Verify all event types present
grep -cE '"name":\s*"(TokenInitialized|AirdropInitialized|AirdropClaimed|AirdropVaultFunded|PresaleInitialized|PresaleBought|PresaleFinalized|PresaleRefunded|VestingInitialized|VestingReleased|VestingRevoked|PoolInitialized|StakingPoolInitialized|RewardsFunded|Staked|Withdrawn|RewardClaimed|StakingPoolPaused|EmergencyWithdrawn)"' \
  target/idl/meme_coin.json
# expected: 19 events
```

---

## 11. Files to delete (workspace Task 15)

```
/home/nhitran/Projects/Solana-Assessment/lib.rs
/home/nhitran/Projects/Solana-Assessment/token.rs
/home/nhitran/Projects/Solana-Assessment/airdrop.rs
/home/nhitran/Projects/Solana-Assessment/presale.rs
/home/nhitran/Projects/Solana-Assessment/dev_fund.rs
/home/nhitran/Projects/Solana-Assessment/liquidity.rs
```

These are reference drafts, not wired into the Anchor program. Will be deleted in workspace Task 15 Step 3.

---

## 12. Open questions / risks (consolidated)

| Risk | Mitigation |
|---|---|
| **SPL `3.0` vs `4.x` version split** — workspace Cargo.toml pins `spl-token = "3.0"` + `spl-token-2022 = "3.0"` but every module plan body (token/airdrop/presale/vesting/liquidity/staking) uses `4.x` API surface (`spl_token::state::Mint::LEN`, `spl_token_2022::instruction::create_mint_with_transfer_fee`). | **Must resolve before any module compiles.** Two paths: (a) bump workspace `Cargo.toml` to `4.x` and verify no breakage in workspace Tasks 1-9, or (b) rewrite module plan code samples to `3.0` API. Recommend (a) — `4.x` is current, `3.0` is legacy. |
| Metaplex CPI API drift between `mpl-token-metadata` versions | Plan has fallback: `mpl_token_metadata::instruction::create_metadata_account` raw builder |
| Raydium CPMM SDK API drift | Pin `0.4`; integration test gated on Task 3 |
| Keypair loss = IDL migration | CI regenerates; store pubkey in safe location |
| `init_if_needed` races | Document in review; case-by-case |
| `MAX_RELEASE_POINTS = 256` ceiling | Documented as production tunable |
| Sol_mem_bot workflow merge with assessment branch | Already addressed (recent commits show `chore(ci): point sol-mem-bot at main branch (preparing assessment deletion)`) |

---

`★ Insight ─────────────────────────────────────`
The workspace plan's hostile-extension guard (Tasks 10-11) is the **critical unblocker** for every module plan. Without `common::assert_no_hostile_extensions`, all 6 module handlers fail to compile. Workspace must finish first.

Staking math test = the cheapest sanity check across all plans (no ProgramTest boot, no transaction signing). Land it first as a smoke test for the rest of the staking module.
`─────────────────────────────────────────────────`

---

## Appendix A — full file structure after build

```
/home/nhitran/Projects/Solana-Assessment/
├── Anchor.toml
├── Cargo.toml
├── rust-toolchain.toml
├── package.json
├── .github/workflows/ci.yml
├── docs/
│   ├── indexer-webhooks.md
│   └── superpowers/
│       ├── plans/  (7 existing plan files)
│       └── 2026-08-29-rust-sdk-deep-dive.md  (THIS FILE)
├── programs/
│   └── meme-coin/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # declare_id! + module wiring
│           ├── common.rs           # HOSTILE_EXTENSIONS + assert_no_hostile_extensions
│           ├── errors.rs           # single ErrorCode enum
│           ├── events.rs           # all #[event] types
│           ├── state/mod.rs        # namespace
│           ├── token/
│           │   ├── mod.rs
│           │   ├── constants.rs
│           │   ├── state.rs        # TokenConfig
│           │   ├── initialize.rs   # InitializeToken
│           │   └── program.rs      # initialize_token handler
│           ├── airdrop/
│           │   ├── mod.rs
│           │   ├── constants.rs
│           │   ├── state.rs        # AirdropState, ClaimAccount
│           │   ├── merkle.rs       # verify_proof, leaf_hash
│           │   ├── initialize.rs
│           │   ├── fund.rs
│           │   ├── claim.rs
│           │   └── program.rs
│           ├── presale/
│           │   ├── mod.rs
│           │   ├── constants.rs
│           │   ├── state.rs        # PresaleState, ContributionAccount, TierPrice
│           │   ├── tiers.rs        # price_for_next_token, max_buyable_in_tier
│           │   ├── initialize.rs
│           │   ├── buy.rs
│           │   ├── finalize.rs
│           │   ├── refund.rs
│           │   └── program.rs
│           ├── vesting/
│           │   ├── mod.rs
│           │   ├── constants.rs
│           │   ├── state.rs        # VestingState, ReleasePoint
│           │   ├── initialize.rs
│           │   ├── release.rs
│           │   ├── revoke.rs
│           │   └── program.rs
│           ├── liquidity/
│           │   ├── mod.rs
│           │   ├── constants.rs    # RAYDIUM_CPMM_PROGRAM_ID
│           │   ├── state.rs        # PoolState
│           │   ├── initialize.rs
│           │   └── program.rs      # initialize_pool handler
│           └── staking/
│               ├── mod.rs
│               ├── constants.rs    # ONE constant
│               ├── state.rs        # PoolState, StakeEntry
│               ├── math.rs         # settle_pending_rewards, pending_reward
│               ├── init_pool.rs
│               ├── fund.rs
│               ├── stake.rs
│               ├── withdraw.rs
│               ├── claim.rs
│               ├── emergency.rs
│               └── program.rs
└── tests/
    ├── Cargo.toml
    └── src/
        ├── lib.rs                  # test root
        ├── helpers.rs              # mint fixtures (later plans)
        ├── hostile_extensions.rs   # workspace Task 10-11
        ├── token_init.rs
        ├── airdrop.rs
        ├── presale.rs
        ├── vesting.rs
        ├── liquidity.rs
        └── staking.rs
```
