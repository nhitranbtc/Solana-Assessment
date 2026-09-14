use anchor_lang::prelude::*;
use anchor_spl::token::spl_token::solana_program::program_option::COption;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{mint_to, Mint, MintTo, Token, TokenAccount},
};
use mpl_token_metadata::{instructions::CreateMetadataAccountV3CpiAccounts, types::DataV2};

pub use crate::state::token_config::TokenConfig;

/// Default fixed supply: 1 billion tokens at 9 decimals.
pub const DEFAULT_INITIAL_SUPPLY: u64 = 1_000_000_000 * 1_000_000_000;
pub const DEFAULT_DECIMALS: u8 = 9;
pub const TOKEN_NAME: &str = "Meme Coin";
pub const TOKEN_SYMBOL: &str = "MEME";
pub const TOKEN_URI: &str = "https://example.com/meme-coin.json";

/// Initialize the SPL mint + global config PDA + treasury token account.
///
/// One-shot: caller becomes program-wide `authority`. Mint authority is the
/// `config` PDA so all subsequent mint ops go through program CPI; admin can
/// later call `renounce_mint_authority` to set it to None permanently.
#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = TokenConfig::SIZE,
        seeds = [b"config"],
        bump,
    )]
    pub config: Account<'info, TokenConfig>,

    #[account(
        init,
        payer = payer,
        mint::decimals = DEFAULT_DECIMALS,
        mint::authority = config,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = config,
    )]
    pub treasury: Account<'info, TokenAccount>,

    /// CHECK: PDA derived externally by Metaplex program; allocated by Metaplex's
    /// CreateMetadataAccountV3 CPI (not by this program).
    #[account(
        seeds = [b"metadata", metadata_program.key().as_ref(), mint.key().as_ref()],
        bump,
        seeds::program = metadata_program.key(),
    )]
    pub metadata: AccountInfo<'info>,

    /// CHECK: validated by address == mpl_token_metadata::ID.
    #[account(
        address = mpl_token_metadata::ID,
    )]
    pub metadata_program: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn initialize_mint(ctx: Context<InitializeMint>) -> Result<()> {
    let cfg = &mut ctx.accounts.config;
    cfg.mint = ctx.accounts.mint.key();
    cfg.decimals = DEFAULT_DECIMALS;
    cfg.initial_supply = DEFAULT_INITIAL_SUPPLY;
    cfg.authority = ctx.accounts.payer.key();
    cfg.mint_authority_renounced = false;
    cfg.bump = ctx.bumps.config;

    // Mint full initial supply to treasury. Config PDA signs via seeds.
    let cfg_seeds: &[&[u8]] = &[b"config", &[cfg.bump]];
    let signer_seeds = &[cfg_seeds];

    let cpi_accounts = MintTo {
        mint: ctx.accounts.mint.to_account_info(),
        to: ctx.accounts.treasury.to_account_info(),
        authority: ctx.accounts.config.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
        signer_seeds,
    );
    mint_to(cpi_ctx, DEFAULT_INITIAL_SUPPLY)?;

    // Create Metaplex metadata account. Config PDA signs as both mint_authority
    // and update_authority so all future metadata updates go through the program.
    let data = DataV2 {
        name: TOKEN_NAME.to_string(),
        symbol: TOKEN_SYMBOL.to_string(),
        uri: TOKEN_URI.to_string(),
        seller_fee_basis_points: 0,
        creators: None,
        collection: None,
        uses: None,
    };
    let metadata_accounts = CreateMetadataAccountV3CpiAccounts {
        metadata: &ctx.accounts.metadata,
        mint: &ctx.accounts.mint.to_account_info(),
        mint_authority: &ctx.accounts.config.to_account_info(),
        payer: &ctx.accounts.payer.to_account_info(),
        update_authority: (&ctx.accounts.config.to_account_info(), true),
        system_program: &ctx.accounts.system_program.to_account_info(),
        rent: Some(&ctx.accounts.rent.to_account_info()),
    };
    let metadata_cpi = mpl_token_metadata::instructions::CreateMetadataAccountV3Cpi::new(
        &ctx.accounts.metadata_program,
        metadata_accounts,
        mpl_token_metadata::instructions::CreateMetadataAccountV3InstructionArgs {
            data,
            is_mutable: true,
            collection_details: None,
        },
    );
    metadata_cpi.invoke_signed(signer_seeds)?;

    Ok(())
}

/// Renounce mint authority. Admin-only, one-shot, irreversible.
#[derive(Accounts)]
pub struct RenounceMintAuthority<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        constraint = !config.mint_authority_renounced @ ErrorCode::AlreadyRenounced,
        constraint = admin.key() == config.authority @ ErrorCode::Unauthorized,
    )]
    pub config: Account<'info, TokenConfig>,

    #[account(
        mut,
        address = config.mint @ ErrorCode::WrongMint,
        constraint = mint.mint_authority == COption::Some(config.key()) @ crate::errors::ErrorCode::MintAuthorityMismatch,
    )]
    pub mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
}

pub fn renounce_mint_authority(ctx: Context<RenounceMintAuthority>) -> Result<()> {
    let bump = ctx.accounts.config.bump;
    let cfg_seeds: &[&[u8]] = &[b"config", &[bump]];
    let signer_seeds = &[cfg_seeds];

    let cpi_accounts = anchor_spl::token::SetAuthority {
        account_or_mint: ctx.accounts.mint.to_account_info(),
        current_authority: ctx.accounts.config.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
        signer_seeds,
    );
    anchor_spl::token::set_authority(
        cpi_ctx,
        anchor_spl::token::spl_token::instruction::AuthorityType::MintTokens,
        None,
    )?;

    ctx.accounts.config.mint_authority_renounced = true;
    Ok(())
}

#[error_code]
pub enum ErrorCode {
    #[msg("Mint authority has already been renounced")]
    AlreadyRenounced,
    #[msg("Signer is not the configured admin authority")]
    Unauthorized,
    #[msg("Mint account does not match the program's configured mint")]
    WrongMint,
}
