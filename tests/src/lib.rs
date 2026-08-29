// /home/nhitran/Projects/Solana-Assessment/tests/src/lib.rs
// Populated by subsequent plans (token, airdrop, presale, vesting, liquidity, staking).
#![allow(dead_code)]

#[test]
fn error_codes_are_exported() {
    let _: fn() -> anchor_lang::Result<()> =
        || Err(meme_coin::errors::ErrorCode::HostileMintExtension.into());
}
