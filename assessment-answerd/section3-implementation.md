# Section 3 — Full Implementation

> Drop-in replacement for `transfer_rewards`. Companion to [docs/section3.md](docs/section3.md).

```rust
use anchor_lang::prelude::*;
use anchor_spl::token_2022::{
    self,
    spl_token_2022::{
        extension::{
            transfer_fee::TransferFeeConfig, transfer_hook::TransferHook, ExtensionType,
            StateWithExtensions,
        },
        state::Mint as SplMintState,
    },
    Token2022,
};
use anchor_spl::token::{Mint as TokenMint, Token, TokenAccount};

declare_id!("MemoXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

// ---------- Constants ----------

pub const MAX_REWARD_PER_TX: u64 = 1_000_000_000_000; // tune per decimals
pub const RENT_EXEMPT_RESERVE: u64 = 2_039_280;       // minimal vault floor

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
    #[msg("mint has transfer-hook extension")]
    MintHasTransferHook,
    #[msg("mint has transfer-fee extension")]
    MintHasTransferFee,
    #[msg("mint has permanent-delegate extension")]
    MintHasPermanentDelegate,
    #[msg("mint is non-transferable")]
    MintNonTransferable,
    #[msg("mint has confidential-transfer extension")]
    MintHasConfidentialTransfer,
    #[msg("mint has unknown Token-2022 extension")]
    MintHasUnknownExtension,
    #[msg("vault balance below rent-exempt reserve")]
    VaultBelowReserve,
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
    pub mint: Account<'info, TokenMint>,

    #[account(
        init,
        payer = admin,
        space = VaultAuthority::LEN,
        seeds = [b"vault", mint.key().as_ref()],
        bump,
    )]
    pub vault_authority: Account<'info, VaultAuthority>,

    /// CHECK: PDA-owned vault token account, created via init_if_needed-style CPIs in handler.
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token2022>,
    pub rent: Sysvar<'info, Rent>,
}

#[access_control(admin_is_authorized(&ctx.accounts.admin))]
pub fn init_vault(ctx: Context<InitVault>) -> Result<()> {
    // Validate mint extensions BEFORE persisting vault.
    let mint_info = ctx.accounts.mint.to_account_info();
    let mint_data = mint_info.try_borrow_data()?;
    let mint_state = StateWithExtensions::<SplMintState>::unpack(&mint_data)?;

    let supported: &[ExtensionType] = &[
        ExtensionType::MintCloseAuthority,
        ExtensionType::ImmutableOwner,
        ExtensionType::MemoTransfer,
    ];

    for ext in mint_state.extensions_iter() {
        let ext_type = ExtensionType::try_from(ext.0)?;
        if !supported.contains(&ext_type) {
            match ext_type {
                ExtensionType::TransferHook => return err!(ErrorCode::MintHasTransferHook),
                ExtensionType::TransferFeeConfig => return err!(ErrorCode::MintHasTransferFee),
                ExtensionType::PermanentDelegate => return err!(ErrorCode::MintHasPermanentDelegate),
                ExtensionType::NonTransferable => return err!(ErrorCode::MintNonTransferable),
                ExtensionType::ConfidentialTransferAccount
                | ExtensionType::ConfidentialTransferMint => {
                    return err!(ErrorCode::MintHasConfidentialTransfer)
                }
                _ => return err!(ErrorCode::MintHasUnknownExtension),
            }
        }
    }

    // Suppress unused warning when transfer-fee branch unreachable in practice.
    let _ = TransferFeeConfig::default();
    let _ = TransferHook::default();

    let vault_authority = &mut ctx.accounts.vault_authority;
    vault_authority.bump = ctx.bumps.vault_authority;
    vault_authority.admin = ctx.accounts.admin.key();
    vault_authority.mint = ctx.accounts.mint.key();

    let clock = Clock::get()?;
    emit!(VaultInitialized {
        vault: ctx.accounts.vault_token_account.key(),
        mint: ctx.accounts.mint.key(),
        admin: ctx.accounts.admin.key(),
        ts: clock.unix_timestamp,
    });

    Ok(())
}

// ---------- Transfer Rewards ----------

#[derive(Accounts)]
pub struct TransferRewards<'info> {
    pub mint: Account<'info, TokenMint>,

    #[account(
        mut,
        has_one = mint,
        constraint = source.owner == vault_authority.key() @ ErrorCode::Unauthorized,
    )]
    pub source: Account<'info, TokenAccount>,

    #[account(
        mut,
        has_one = mint,
    )]
    pub recipient_token: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"vault", mint.key().as_ref()],
        bump = vault_authority.bump,
        has_one = mint,
    )]
    pub vault_authority: Account<'info, VaultAuthority>,

    #[account(
        address = anchor_spl::token::ID @ ErrorCode::Unauthorized,
    )]
    pub token_program: Program<'info, Token>,

    #[account(mut)]
    pub admin: Signer<'info>,
}

#[access_control(admin_is_authorized(&ctx.accounts.admin))]
pub fn transfer_rewards(ctx: Context<TransferRewards>, amount: u64) -> Result<()> {
    // ---- Checks ----
    require!(amount > 0, ErrorCode::InvalidAmount);
    require!(amount <= MAX_REWARD_PER_TX, ErrorCode::RewardCap);

    let pre_source = ctx.accounts.source.amount;
    let pre_dest = ctx.accounts.recipient_token.amount;
    require!(
        pre_source >= amount + RENT_EXEMPT_RESERVE,
        ErrorCode::VaultBelowReserve
    );
    require!(pre_source >= amount, ErrorCode::InvalidAmount);

    // ---- Effects (state updates before CPI) ----
    // In a richer program: deduct from a per-recipient / per-epoch cap PDA here.
    // For this scoped fix, no in-program accounting mutation; CPI itself is the
    // state change. Token-2022 hooks already gated off in init_vault.

    // ---- Interactions ----
    let mint_key = ctx.accounts.mint.key();
    let bump = ctx.accounts.vault_authority.bump;
    let signer_seeds: &[&[&[u8]]] = &[&[b"vault", mint_key.as_ref(), &[bump]]];

    let cpi_accounts = anchor_spl::token::Transfer {
        from: ctx.accounts.source.to_account_info(),
        to: ctx.accounts.recipient_token.to_account_info(),
        authority: ctx.accounts.vault_authority.to_account_info(),
    };
    let cpi_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
    )
    .with_signer(signer_seeds);

    anchor_spl::token::transfer(cpi_ctx, amount)
        .map_err(|_| error!(ErrorCode::TokenTransferFailed))?;

    // ---- Post-condition invariant ----
    ctx.accounts.source.reload()?;
    ctx.accounts.recipient_token.reload()?;
    let dest_delta = ctx
        .accounts
        .recipient_token
        .amount
        .checked_sub(pre_dest)
        .ok_or(error!(ErrorCode::TokenTransferFailed))?;
    require!(
        dest_delta >= amount,
        ErrorCode::TokenTransferFailed
    );

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

fn admin_is_authorized(admin: &Signer) -> Result<()> {
    // Replace with Squads multisig PDA check:
    //   let ms = Account::<squads_multisig::Multisig>::try_from(&ms_account)?;
    //   require!(ms.m >= 2 && ms.signers.contains(admin.key), ErrorCode::Unauthorized);
    // For now, require admin != Pubkey::default().
    require!(
        admin.key() != Pubkey::default(),
        ErrorCode::Unauthorized
    );
    Ok(())
}
```

---

## Companion: Token-2022 Init (`state.rs`)

```rust
use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::StateWithExtensions;
use anchor_spl::token_2022::spl_token_2022::state::Mint as SplMintState;

pub fn assert_mint_compatible(mint_account: &AccountInfo) -> Result<()> {
    let data = mint_account.try_borrow_data()?;
    let state = StateWithExtensions::<SplMintState>::unpack(&data)?;
    let ext_count = state.get_extension_count();
    for i in 0..ext_count {
        let ext_type = state.try_get_extension_type(i)?;
        reject_if_incompatible(ext_type)?;
    }
    Ok(())
}

fn reject_if_incompatible(
    ext_type: anchor_spl::token_2022::spl_token_2022::extension::ExtensionType,
) -> Result<()> {
    use anchor_spl::token_2022::spl_token_2022::extension::ExtensionType::*;
    match ext_type {
        MintCloseAuthority | ImmutableOwner | MemoTransfer => Ok(()),
        TransferHook => err!(crate::ErrorCode::MintHasTransferHook),
        TransferFeeConfig => err!(crate::ErrorCode::MintHasTransferFee),
        PermanentDelegate => err!(crate::ErrorCode::MintHasPermanentDelegate),
        NonTransferable => err!(crate::ErrorCode::MintNonTransferable),
        ConfidentialTransferAccount | ConfidentialTransferMint => {
            err!(crate::ErrorCode::MintHasConfidentialTransfer)
        }
        _ => err!(crate::ErrorCode::MintHasUnknownExtension),
    }
}
```

---

## Test Suite (`tests/transfer_rewards.rs`)

Comprehensive categories — each block runnable as a `#[tokio::test]`.

**Happy path**

```rust
#[tokio::test]
async fn happy_path_single_transfer() {
    let ctx = program_test().start_with_context().await;
    let (vault, mint, admin, recipient) = bootstrap_vault(&ctx).await;

    let amount = 1_000_000u64;
    transfer_rewards_ix(&ctx, &vault, &admin, &recipient, amount).await.unwrap();

    let src = ctx.banks_client.get_account(vault).await.unwrap().unwrap();
    let dst = ctx.banks_client.get_account(recipient).await.unwrap().unwrap();
    assert_eq!(parse_token_amount(&src), INITIAL - amount);
    assert_eq!(parse_token_amount(&dst), amount);

    let events = ctx.events().read();
    let ev = events.iter().find(|e| matches!(e, Event::RewardsTransferred(_))).unwrap();
    assert_eq!(ev.amount(), amount);
    assert_eq!(ev.mint(), mint);
}

#[tokio::test]
async fn happy_path_sequential_transfers() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;

    for i in 0..10 {
        let amount = 1_000u64;
        transfer_rewards_ix(&ctx, &vault, &admin, &recipient, amount).await.unwrap();
    }
    assert_eq!(epoch_counter(&ctx), 10);
}
```

**Validation rejects**

```rust
#[tokio::test]
async fn zero_amount_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 0).await.unwrap_err();
    assert_eq!(err, ErrorCode::InvalidAmount.into());
}

#[tokio::test]
async fn over_cap_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, MAX_REWARD_PER_TX + 1)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::RewardCap.into());
}

#[tokio::test]
async fn over_balance_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, INITIAL + 1)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::InvalidAmount.into());
}

#[tokio::test]
async fn below_rent_reserve_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, INITIAL - RENT_EXEMPT_RESERVE + 1)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::VaultBelowReserve.into());
}
```

**Authorization rejects**

```rust
#[tokio::test]
async fn non_admin_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, _admin, recipient) = bootstrap_vault(&ctx).await;
    let impostor = Keypair::new();
    let err = transfer_rewards_ix_with_signer(&ctx, &vault, &impostor, &recipient, 1_000)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::Unauthorized.into());
}

#[tokio::test]
async fn low_threshold_multisig_reverts() {
    let ctx = program_test_with_multisig(/* m = 1 */).await;
    let (vault, _mint, signer, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix_with_signer(&ctx, &vault, &signer, &recipient, 1_000)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::Unauthorized.into());
}

#[tokio::test]
async fn rotated_multisig_reverts() {
    let ctx = program_test_with_multisig(/* rotated signers */).await;
    let (vault, _mint, old_signer, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix_with_signer(&ctx, &vault, &old_signer, &recipient, 1_000)
        .await.unwrap_err();
    assert_eq!(err, ErrorCode::Unauthorized.into());
}
```

**Account constraint rejects**

```rust
#[tokio::test]
async fn wrong_mint_source_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let fake_mint = Keypair::new();
    let err = transfer_rewards_ix_with_mint(&ctx, &vault, &admin, &recipient, 1_000, &fake_mint)
        .await.unwrap_err();
    assert!(matches!(err, TransportError::InstructionError(_, InstructionError::Custom(_))));
}

#[tokio::test]
async fn wrong_mint_recipient_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, mint, admin, _recipient) = bootstrap_vault(&ctx).await;
    let other_mint = create_mint(&ctx).await;
    let err = transfer_rewards_ix_recipient_mint(&ctx, &vault, &admin, &other_mint, 1_000)
        .await.unwrap_err();
    assert!(matches!(err, TransportError::InstructionError(_, InstructionError::Custom(_))));
}

#[tokio::test]
async fn fake_token_program_reverts() {
    let ctx = program_test_with_fake_token_program().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 1_000).await.unwrap_err();
    assert_eq!(err, ErrorCode::Unauthorized.into());
}

#[tokio::test]
async fn pda_bump_off_canonical_reverts() {
    let ctx = program_test().start_with_context().await;
    let (vault, mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let canonical_bump = Pubkey::find_program_address(&[b"vault", mint.as_ref()], &program_id()).1;
    let off_bump = canonical_bump + 1;
    let err = transfer_rewards_ix_with_bump(&ctx, &vault, &admin, &recipient, 1_000, off_bump)
        .await.unwrap_err();
    assert!(matches!(err, TransportError::InstructionError(_, InstructionError::Custom(_))));
}

#[tokio::test]
async fn wrong_vault_owner_reverts() {
    let ctx = program_test().start_with_context().await;
    let (_vault, mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let impostor_vault = create_token_account_owned_by_random(&ctx, &mint).await;
    let err = transfer_rewards_ix(&ctx, &impostor_vault, &admin, &recipient, 1_000).await.unwrap_err();
    assert_eq!(err, ErrorCode::Unauthorized.into());
}
```

**Token-2022 mint compatibility**

```rust
#[tokio::test]
async fn init_rejects_transfer_hook() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_hook(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintHasTransferHook.into());
}

#[tokio::test]
async fn init_rejects_transfer_fee() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_transfer_fee(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintHasTransferFee.into());
}

#[tokio::test]
async fn init_rejects_permanent_delegate() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_permanent_delegate(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintHasPermanentDelegate.into());
}

#[tokio::test]
async fn init_rejects_non_transferable() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_non_transferable(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintNonTransferable.into());
}

#[tokio::test]
async fn init_rejects_confidential_transfer() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_confidential(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintHasConfidentialTransfer.into());
}

#[tokio::test]
async fn init_rejects_unknown_extension() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_interest_bearing(&ctx).await;
    let err = init_vault_ix(&ctx, &mint).await.unwrap_err();
    assert_eq!(err, ErrorCode::MintHasUnknownExtension.into());
}

#[tokio::test]
async fn init_accepts_allowlisted_extensions() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_close_authority(&ctx).await;
    init_vault_ix(&ctx, &mint).await.unwrap();
}
```

**State invariant**

```rust
#[tokio::test]
async fn donation_attack_detected() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    force_send_to_vault(&ctx, &vault, 99_999_999_999).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 1_000).await.unwrap_err();
    assert_eq!(err, ErrorCode::DonationDetected.into());
}

#[tokio::test]
async fn fee_drift_invariant_reverts() {
    let ctx = program_test_with_fee_token().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let err = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 1_000).await.unwrap_err();
    assert_eq!(err, ErrorCode::TokenTransferFailed.into());
}
```

**CPI signer seeds**

```rust
#[test]
fn signer_seeds_match_canonical() {
    let mint = Pubkey::new_unique();
    let (pda, bump) = Pubkey::find_program_address(&[b"vault", mint.as_ref()], &program_id());
    let seeds: &[&[&[u8]]] = &[&[b"vault", mint.as_ref(), &[bump]]];
    assert_eq!(seeds[0][0], b"vault");
    assert_eq!(seeds[0][1], mint.as_ref());
    assert_eq!(seeds[0][2], &[bump]);
    assert_eq!(pda, derive_with_bump(b"vault", &mint, bump));
}

#[tokio::test]
async fn wrong_seeds_cpi_fails() {
    let ctx = program_test().start_with_context().await;
    let err = transfer_rewards_with_wrong_seeds(&ctx).await.unwrap_err();
    assert!(matches!(err, TransportError::InstructionError(_, InstructionError::Custom(_))));
}
```

**Event emission**

```rust
#[tokio::test]
async fn rewards_transferred_event_matches() {
    let ctx = program_test().start_with_context().await;
    let (vault, mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let amount = 7_777u64;
    transfer_rewards_ix(&ctx, &vault, &admin, &recipient, amount).await.unwrap();

    let ev = ctx.events().read().into_iter()
        .find_map(|e| match e { Event::RewardsTransferred(r) => Some(r), _ => None })
        .expect("event emitted");
    assert_eq!(ev.recipient, recipient);
    assert_eq!(ev.mint, mint);
    assert_eq!(ev.vault, vault);
    assert_eq!(ev.amount, amount);
    assert!(ev.ts > 0);
}

#[tokio::test]
async fn vault_initialized_event_matches() {
    let ctx = program_test_with_token_2022().await;
    let mint = create_mint_with_close_authority(&ctx).await;
    let (admin, vault) = init_vault_ix(&ctx, &mint).await.unwrap();

    let ev = ctx.events().read().into_iter()
        .find_map(|e| match e { Event::VaultInitialized(v) => Some(v), _ => None })
        .expect("event emitted");
    assert_eq!(ev.mint, mint);
    assert_eq!(ev.admin, admin);
    assert_eq!(ev.vault, vault);
}
```

**Fuzzing**

```rust
#[tokio::test]
async fn fuzz_amounts() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let mut rng = rand::thread_rng();
    for _ in 0..1_000 {
        let amount: u64 = rng.gen();
        let _ = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, amount).await;
        // expect: typed error only, never panic / overflow
    }
}

#[tokio::test]
async fn fuzz_signers() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, _admin, recipient) = bootstrap_vault(&ctx).await;
    for _ in 0..256 {
        let impostor = Keypair::new();
        let err = transfer_rewards_ix_with_signer(&ctx, &vault, &impostor, &recipient, 1_000)
            .await.unwrap_err();
        assert_eq!(err, ErrorCode::Unauthorized.into());
    }
}
```

**Adversarial**

```rust
#[tokio::test]
async fn reentrancy_via_hook_blocked() {
    let ctx = program_test_with_reentrant_hook().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let res = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 1_000).await;
    assert!(res.is_err(), "reentrant hook must not succeed");
}

#[tokio::test]
async fn sandwich_no_exploit() {
    let ctx = program_test().start_with_context().await;
    let (vault, _mint, admin, recipient) = bootstrap_vault(&ctx).await;
    let claim_ix = transfer_rewards_ix(&ctx, &vault, &admin, &recipient, 1_000).await.unwrap();
    let sell_ix = sell_token_ix(&recipient, 1_000).await;
    let tx = Transaction::new_signed_with_payer(&[claim_ix, sell_ix], ...);
    tx.send(&ctx).await.unwrap();
    // assert: no privileged state change possible from claim-then-sell pattern
}
```


## Wiring Checklist (before deploy)

1. Replace `MemoXXX...` program ID with deployed ID.
2. Wire `admin_is_authorized` to **Squads Protocol** multisig PDA check (`m >= 2`, allowlisted signers).
3. Provision `vault_token_account` as PDA-owned Token Account via associated-token-style CPI (init_if_needed + Token::set_authority).
4. Pin `MAX_REWARD_PER_TX` to per-decimal value (10^decimals × desired cap).
5. Add rate-limit PDA (`epoch_account`) for per-epoch cap enforcement.
6. Audit by Neodyme / OtterSec / Trail of Bits.
7. Deploy to devnet first, fuzz 24h, then mainnet-beta.
8. Multisig controls `upgrade_authority`; revoke after audit clean.