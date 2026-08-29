// /home/nhitran/Projects/Solana-Assessment/programs/meme-coin/src/state/mod.rs
// Shared account layouts live here as the program grows.
// Each downstream module plan adds its own sub-module under this directory.
//
// Example convention (filled in by token / airdrop / presale / vesting / liquidity / staking plans):
//   pub mod token;
//   pub mod airdrop;
//   pub mod presale;
//   pub mod vesting;
//   pub mod liquidity;
//   pub mod staking;

pub use anchor_lang::AccountSerialize as _;
