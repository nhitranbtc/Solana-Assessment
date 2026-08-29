use anchor_lang::prelude::*;

// ===== Token module =====
#[event]
pub struct TokenInitialized {
    #[index]
    pub mint: Pubkey,
    pub decimals: u8,
    pub total_supply: u64,
    pub metadata_pda: Pubkey,
}

// ===== Airdrop module =====
#[event]
pub struct AirdropInitialized {
    #[index]
    pub airdrop: Pubkey,
    pub mint: Pubkey,
    pub start_ts: i64,
    pub end_ts: i64,
    pub total_tokens: u64,
    pub merkle_root: [u8; 32],
}

#[event]
pub struct AirdropClaimed {
    #[index]
    pub user: Pubkey,
    #[index]
    pub airdrop: Pubkey,
    pub amount: u64,
}

#[event]
pub struct AirdropVaultFunded {
    #[index]
    pub airdrop: Pubkey,
    pub amount: u64,
}

// ===== Presale module =====
#[event]
pub struct PresaleInitialized {
    #[index]
    pub presale: Pubkey,
    pub mint: Pubkey,
    pub start_ts: i64,
    pub end_ts: i64,
    pub soft_cap_lamports: u64,
    pub hard_cap_lamports: u64,
}

#[event]
pub struct PresaleBought {
    #[index]
    pub buyer: Pubkey,
    #[index]
    pub presale: Pubkey,
    pub amount: u64,
    pub total_cost_lamports: u64,
}

#[event]
pub struct PresaleFinalized {
    #[index]
    pub presale: Pubkey,
    pub reached_soft_cap: bool,
    pub total_sold: u64,
    pub total_lamports: u64,
}

#[event]
pub struct PresaleRefunded {
    #[index]
    pub buyer: Pubkey,
    #[index]
    pub presale: Pubkey,
    pub lamports: u64,
    pub tokens: u64,
}

// ===== Vesting (dev fund) module =====
#[event]
pub struct VestingInitialized {
    #[index]
    pub vesting: Pubkey,
    #[index]
    pub beneficiary: Pubkey,
    pub total_amount: u64,
    pub cliff_ts: i64,
    pub end_ts: i64,
}

#[event]
pub struct VestingReleased {
    #[index]
    pub vesting: Pubkey,
    #[index]
    pub beneficiary: Pubkey,
    pub amount: u64,
    pub total_released: u64,
}

#[event]
pub struct VestingRevoked {
    #[index]
    pub vesting: Pubkey,
    pub by: Pubkey,
}

// ===== Liquidity module =====
#[event]
pub struct PoolInitialized {
    #[index]
    pub pool_authority: Pubkey,
    pub mint: Pubkey,
    pub token_amount: u64,
    pub sol_amount: u64,
    pub lp_burned: u64,
}

// ===== Staking module =====
#[event]
pub struct StakingPoolInitialized {
    #[index]
    pub pool: Pubkey,
    pub mint: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_rate_per_sec: u64,
    pub lockup_seconds: u64,
}

#[event]
pub struct RewardsFunded {
    #[index]
    pub pool: Pubkey,
    pub amount: u64,
}

#[event]
pub struct Staked {
    #[index]
    pub user: Pubkey,
    #[index]
    pub pool: Pubkey,
    pub amount: u64,
    pub lockup_at_deposit: i64,
}

#[event]
pub struct Withdrawn {
    #[index]
    pub user: Pubkey,
    #[index]
    pub pool: Pubkey,
    pub amount: u64,
}

#[event]
pub struct RewardClaimed {
    #[index]
    pub user: Pubkey,
    #[index]
    pub pool: Pubkey,
    pub amount: u64,
}

#[event]
pub struct StakingPoolPaused {
    #[index]
    pub pool: Pubkey,
    pub by: Pubkey,
}

#[event]
pub struct EmergencyWithdrawn {
    #[index]
    pub user: Pubkey,
    #[index]
    pub pool: Pubkey,
    pub amount: u64,
}
