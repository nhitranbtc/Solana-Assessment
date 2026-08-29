use anchor_lang::prelude::*;

declare_id!("5BNHWNAYTBXL9SUJAwRT2yBMNAtJTt4irpttCiDFYbYB");

#[program]
pub mod meme_coin {
    use super::*;
    pub fn placeholder(_ctx: Context<Placeholder>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Placeholder {}
