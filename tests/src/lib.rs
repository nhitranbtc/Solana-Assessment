// /home/nhitran/Projects/Solana-Assessment/tests/src/lib.rs
// Populated by subsequent plans (token, airdrop, presale, vesting, liquidity, staking).
#![allow(dead_code)]

#[test]
fn error_codes_are_exported() {
    let _: fn() -> anchor_lang::Result<()> =
        || Err(meme_coin::errors::ErrorCode::HostileMintExtension.into());
}

#[test]
fn event_types_are_exported() {
    let _evt: meme_coin::events::TokenInitialized = meme_coin::events::TokenInitialized {
        mint: anchor_lang::solana_program::pubkey::Pubkey::default(),
        decimals: 9,
        total_supply: 0,
        metadata_pda: anchor_lang::solana_program::pubkey::Pubkey::default(),
    };
}
