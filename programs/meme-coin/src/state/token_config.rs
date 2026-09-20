use anchor_lang::prelude::*;

/// Global token config PDA. Seed: `[b"config"]`.
///
/// Holds the SPL mint address and program-wide authority flags. One per program.
/// `mint_authority_renounced` flips true after admin calls `renounce_mint_authority`
/// — irreversible, guards against accidental re-mint.
#[account]
pub struct TokenConfig {
    pub mint: Pubkey,
    pub decimals: u8,
    pub initial_supply: u64,
    pub authority: Pubkey,
    pub mint_authority_renounced: bool,
    pub bump: u8,
}

impl TokenConfig {
    /// Discriminator (8) + mint (32) + decimals (1) + initial_supply (8)
    /// + authority (32) + mint_authority_renounced (1) + bump (1) = 83.
    pub const SIZE: usize = 8 + 32 + 1 + 8 + 32 + 1 + 1;
}
