use anchor_lang::prelude::*;

declare_id!("5BNHWNAYTBXL9SUJAwRT2yBMNAtJTt4irpttCiDFYbYB");

pub mod errors;
pub mod events;
pub mod state;
pub mod token;

pub(crate) use token::__client_accounts_initialize_mint;
pub(crate) use token::__client_accounts_renounce_mint_authority;

#[program]
pub mod meme_coin {
    use super::*;
    pub use crate::token::{InitializeMint, RenounceMintAuthority};

    pub fn initialize_mint(ctx: Context<InitializeMint>) -> Result<()> {
        token::initialize_mint(ctx)
    }

    pub fn renounce_mint_authority(ctx: Context<RenounceMintAuthority>) -> Result<()> {
        token::renounce_mint_authority(ctx)
    }
}

pub use errors::ErrorCode;
