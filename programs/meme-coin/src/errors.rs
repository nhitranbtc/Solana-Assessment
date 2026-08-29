use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    // Token-2022 hostile extensions
    #[msg("Mint carries a hostile Token-2022 extension (transfer-hook, fee, permanent-delegate, non-transferable, confidential, cpi-guard, close-authority, memo-transfer, metadata-pointer, immutable-owner, default-account-state, group-pointer, group-member-pointer).")]
    HostileMintExtension,

    // Authority / signers
    #[msg("Signer is not the configured authority.")]
    Unauthorized,
    #[msg("Multisig threshold not satisfied.")]
    MultisigThresholdNotMet,

    // Arithmetic
    #[msg("Arithmetic overflow.")]
    Overflow,
    #[msg("Arithmetic underflow.")]
    Underflow,
    #[msg("Division by zero.")]
    DivisionByZero,

    // State transitions
    #[msg("Operation paused.")]
    Paused,
    #[msg("Operation not yet enabled.")]
    NotEnabled,
    #[msg("Window not yet open.")]
    TooEarly,
    #[msg("Window already closed.")]
    TooLate,

    // Airdrop
    #[msg("Invalid Merkle proof.")]
    InvalidProof,
    #[msg("Airdrop already claimed by this user.")]
    AlreadyClaimed,

    // Presale
    #[msg("Slippage tolerance exceeded.")]
    SlippageExceeded,
    #[msg("Presale soft cap not reached; refund available.")]
    SoftCapNotReached,
    #[msg("Hard cap exceeded.")]
    HardCapExceeded,

    // Vesting
    #[msg("Cliff not yet reached.")]
    CliffNotReached,
    #[msg("Vesting schedule has been revoked.")]
    VestingRevoked,

    // Staking
    #[msg("Lockup period not yet elapsed.")]
    LockupActive,
    #[msg("Pool is empty; cannot compute per-share reward.")]
    PoolEmpty,
    #[msg("Staker amount exceeds available withdrawable.")]
    InsufficientUnlocked,

    // Account validation
    #[msg("Provided account does not match expected mint.")]
    MintMismatch,
    #[msg("Provided account does not match expected PDA.")]
    PdaMismatch,
    #[msg("Account discriminator mismatch.")]
    DiscriminatorMismatch,
}
