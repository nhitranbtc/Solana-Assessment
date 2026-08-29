use anchor_lang::prelude::*;

declare_id!("5BNHWNAYTBXL9SUJAwRT2yBMNAtJTt4irpttCiDFYbYB");

pub mod errors;
pub mod events;
pub mod state;

#[program]
pub mod meme_coin {
    use super::*;
    pub fn placeholder(_ctx: Context<Placeholder>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Placeholder {}

pub use errors::ErrorCode;
