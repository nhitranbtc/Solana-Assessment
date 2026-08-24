# Section 1 — Solana Fundamentals Deep Dive

> Source: [assessment.txt §1](../assessment.txt#L13). Original brief: 3 questions (Accounts/Programs/PDAs, Anchor vs Native, treasury risks). This deep dive expands each with concrete meme-coin examples, code stubs, comparison tables, and security patterns. Companion to [§1](assessment-answerd.md).

---

## Q1: Accounts vs Programs vs PDAs

### Account

A record on Solana holding **lamports** (SOL), **data** (arbitrary bytes), and an **owner** (program id). Every on-chain artifact — wallet balance, token mint, token balance, config, NFT metadata, program executable — is an account.

```rust
// AccountInfo (raw view)
pub struct AccountInfo {
    pub key: Pubkey,          // 32-byte address
    pub lamports: u64,        // SOL balance
    pub data: RefCell<&mut [u8]>,  // arbitrary bytes (often deserialized via Anchor)
    pub owner: Pubkey,        // program id that owns this account
    pub rent_epoch: Epoch,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,     // true → executable account = program
}
```

**Account categories in a meme-coin project:**

| Account | Owner | Holds | Mutability |
|---------|-------|-------|-----------|
| Meme mint | Token Program (`TokenkegQ...`) | supply, decimals, mint authority, freeze authority | never writable by users; only Token Program mutates |
| User wallet (system account) | System Program | lamports | writable by owner |
| User ATA (associated token account) | Token Program | token balance, owner = user's wallet | writable by Token Program |
| Treasury PDA (token account) | Meme Program | token balance, owner = treasury PDA | writable by Token Program + signed by Meme Program |
| Config PDA | Meme Program | global config (fees, paused, authorities) | writable by Meme Program only |

**Meme-coin example:** the SPL Token mint for a meme coin is an account owned by the SPL Token Program. The mint account data holds `supply` (u64), `decimals` (u8), `is_initialized` (bool), `mint_authority` (Option<Pubkey>), `freeze_authority` (Option<Pubkey>). The mint authority (if set) is the only address that can `mint_to` new supply.

### Program

Stateless executable bytecode (Berkeley Packet Filter / BPF) deployed to an account marked `executable: true`. Programs are owned by the BPF Loader (or Loader-v2/v3/v4) and cannot mutate their own data after deployment (unless `upgrade_authority` is set on a Loader-v2/v3 upgradeable program).

```rust
#[program]
pub mod meme_coin {
    use super::*;

    pub fn create_mint(ctx: Context<CreateMint>, decimals: u8) -> Result<()> {
        // CPI to token::initialize_mint
        Ok(())
    }

    pub fn transfer_with_fee(ctx: Context<TransferWithFee>, amount: u64) -> Result<()> {
        // Validate, then CPI to token::transfer or token::transfer_checked
        Ok(())
    }

    pub fn burn(ctx: Context<Burn>, amount: u64) -> Result<()> {
        // Validate, then CPI to token::burn
        Ok(())
    }
}
```

**Meme-coin example:** the `meme_coin` program with three instructions (`create_mint`, `transfer_with_fee`, `burn`). Each instruction is an entry point identified by an 8-byte discriminator (the first 8 bytes of `Sha256("global:<instruction_name>")`).

### PDAs (Program Derived Addresses)

Deterministic addresses derived from `(program_id, seeds[])` that have **no private key** on the Ed25519 curve. Only the program that owns the PDA can sign for it via `invoke_signed`. The off-curve guarantee comes from bumping a candidate seed until the resulting address falls off the Ed25519 curve.

```rust
// Derivation
let (pda, bump) = Pubkey::find_program_address(&[b"treasury", mint_key.as_ref()], &program_id);

// Signing in CPI
// signer_seeds shape: outer list = multiple signer groups (rare),
// middle list = one group of seed slices for one signing PDA,
// innermost bytes = the seed bytes themselves (bump is the last element of the seeds list).
let signer_seeds: &[&[&[u8]]] = &[&[
    b"treasury",
    mint_key.as_ref(),
    &[bump],
]];
invoke_signed(&instruction, accounts, &[signer_seeds])?;
```

**Meme-coin PDAs:**

| PDA | Seeds | Authority for | Purpose |
|-----|-------|---------------|---------|
| Treasury | `[b"treasury", mint_key]` | treasury token account | holds LP seed, presale proceeds, team allocation; signs CPI to transfer tokens out |
| Config | `[b"config", mint_key]` | global config | stores fees, paused flag, multisig reference |
| Staking pool | `[b"pool", mint_key]` | staking/reward vaults | MasterChef accumulator + state |
| Stake | `[b"stake", pool_key, user_key]` | user stake state | per-user position |
| Authority (admin) | `[b"authority", mint_key]` | upgrade authority / migration | controls program upgrades + migration |

**Why PDAs matter:** with a keypair authority, the private key is a single point of compromise — leak it and the treasury drains. With a PDA authority, no key exists; only the program can sign for the PDA via `invoke_signed`. Combined with multisig-controlled upgrade authority, the program logic becomes the trust anchor.

> **Insight:** PDAs solve the single biggest Solana foot-gun — "who holds authority?". A keypair authority is a footgun; a PDA authority is the canonical pattern.

---

## Q2: Anchor Framework vs Native Solana

### Decision matrix

| Dimension | Anchor | Native |
|-----------|--------|--------|
| Development speed | fast (macros, IDL gen) | slow (manual account parsing, manual discriminator) |
| Account validation | declarative (`#[account]`, `has_one`, `constraint`, `init`, `mut`, `close`) | imperative (`if account.owner != expected_program → err!`) |
| IDL | auto-generated from source | manual JSON authoring |
| Client SDK | auto-generated TypeScript client | hand-roll or third-party |
| Error handling | `#[error_code]` enum with typed variants | raw `ProgramError::Custom(u32)` |
| CPI ergonomics | `CpiContext::new` / `with_signer` with typed accounts | raw `invoke` / `invoke_signed` with `AccountInfo` slices |
| Compute Units | higher baseline (simple constraints ~10-200 CU each; `init` adds ~5000+ CU for System Program CPI) | lower (manual = no overhead) |
| Program binary size | larger (Anchor runtime + macros expand) | smaller |
| Custom layouts / zero-copy | awkward | natural |
| Idiomatic Rust ownership | obscured by macro expansion | direct |

### Pick Anchor when

- Ship fast, fewer foot-guns
- Need IDL for client gen + TS SDK
- Standard account patterns (init, mutate, close)
- CPI ergonomics matter (token program, associated token, other Anchor programs)

### Pick native when

- Maximum CU optimization needed (Anchor constraints add compute)
- Custom account layouts / bitfields / zero-copy
- Program size limit tight (Anchor bloat)
- Full control over discriminator / serialization

### Concrete meme-coin example: `create_meme_coin` instruction

**Anchor (one attribute, ~15 lines):**

```rust
#[derive(Accounts)]
pub struct CreateMemeCoin<'info> {
    #[account(
        init,
        payer = user,
        mint::decimals = 9,
        mint::authority = treasury_authority,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = user,
        token::mint = mint,
        token::authority = treasury_authority,
    )]
    pub treasury: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"treasury", mint.key().as_ref()],
        bump,
    )]
    pub treasury_authority: AccountInfo<'info>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn create_meme_coin(ctx: Context<CreateMemeCoin>) -> Result<()> {
    // Mints initial supply to treasury in CPI; config PDA initialized separately.
    Ok(())
}
```

**Native (~30 lines, manual owner/length/mint checks):**

```rust
pub fn create_meme_coin(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let user = &accounts[0];
    let mint = &accounts[1];
    let treasury = &accounts[2];
    let treasury_authority = &accounts[3];
    let system_program = &accounts[4];
    let token_program = &accounts[5];
    let rent = &accounts[6];

    // Manual signer check
    if !user.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    // Manual owner check (mint account must be owned by Token Program)
    if mint.owner != spl_token::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    // Manual length check (mint account must be 82 bytes)
    if mint.data_len() != 82 {
        return Err(ProgramError::InvalidAccountData);
    }
    // Manual rent check
    let rent_data = Rent::from_account_info(rent)?;
    if !rent_data.is_exempt(mint.lamports(), mint.data_len()) {
        return Err(ProgramError::AccountNotRentExempt);
    }
    // Manual PDA derivation + canonicalization
    let (expected_pda, bump) = Pubkey::find_program_address(
        &[b"treasury", mint.key.as_ref()],
        program_id,
    );
    if expected_pda != *treasury_authority.key {
        return Err(ProgramError::InvalidSeeds);
    }
    // Manual invoke_signed for initialize_mint2 CPI
    let ix = spl_token::instruction::initialize_mint2(
        &spl_token::ID,
        mint.key,
        treasury_authority.key,
        None, // freeze authority
        9,
    )?;
    invoke_signed(&ix, &[mint], &[&[b"treasury", mint.key.as_ref(), &[bump]]])?;
    Ok(())
}
```

**Verdict for meme coin at MVP:** Anchor wins 10/10. Revisit native only if CU becomes the bottleneck.

---

## Q3: Token Treasury Security Risks

### Risk inventory (with meme-coin framing)

| Risk | Meme-coin impact | Mitigation |
|------|------------------|------------|
| Authority compromise | LP seed + presale SOL drained in one tx before any holder can sell | PDA authority + Squads multisig (m ≥ 2) |
| Single signer / no multisig | Presale vault can be rugged unilaterally by admin key holder | Squads m ≥ 2 for treasury moves |
| Unchecked mint authority | Infinite mint dilutes holders; cap-table collapses | Revoke mint authority (or transfer to PDA) AND revoke freeze authority separately |
| Unbounded token-rewards transfer | CPI reentrancy via malicious Token-2022 transfer-hook calls back into our instruction | Refuse Token-2022 mints with hostile extensions; cap per-tx; PDA `is_locked` flag |
| Account substitution | Wrong token account passed as vault; funds leak to attacker-supplied account | `has_one = mint` + owner check on every CPI |
| Missing account validation | Owner / key / mint / decimals mismatch enable silent value loss | Required on every CPI account set; `address = <known_pubkey>` constraints |
| Arithmetic overflow in fee/swap math | Integer overflow yields free tokens or DoS | `checked_mul` / `checked_add` / `checked_sub` everywhere |
| Rent exemption not ensured | Accounts garbage-collected, state lost | Anchor `#[account(init, ...)]` handles; explicit `Rent` sysvar check elsewhere |
| Upgrade authority not revoked / time-locked | Malicious program upgrade drains all vaults | Squads controls upgrade; timelock; revoke after audit |
| Token-2022 hostile extensions (transfer-hook / fee / permanent-delegate / non-transferable / confidential) | Reentrancy, value leak, vault drain, DoS | Refuse at vault init (TLV-walk the mint) |
| PDA bump not canonicalized | Wrong seeds → wrong authority signed → silent transfer failure | `#[account(seeds, bump)]` + reuse stored bump in CPI signer seeds |
| PDA vault ownership unverified | Source token account owned by random program → funds leak | `constraint = source.owner == vault_authority.key()` |
| Missing event emission | Indexers blind, audit trail missing, no proof-of-reserve | `emit!` typed events on every state transition |
| MEV / sandwich on token launches | Bots front-run buys, holders eat slippage | Private mempool submission, batched launches, anti-bot cooldown |
| Multisig gap (sub-threshold signers execute) | Even with multisig, m=1 is one compromised signer away from drain | Squads m ≥ 2 with allowlisted signer set; time-locked execution |

### Defensive patterns

```rust
// PDA authority + multisig
#[account(seeds = [b"treasury", mint.key().as_ref()], bump)]
pub treasury_authority: AccountInfo<'info>,

// Ownership + mint pin
#[account(
    mut,
    has_one = mint,
    constraint = vault.owner == treasury_authority.key(),
)]
pub vault: Account<'info, TokenAccount>,

// Refuse Token-2022 hostile extensions at vault init
fn reject_hostile_mint_extensions(mint_info: &AccountInfo) -> Result<()> {
    // TLV walk; reject transfer-hook, transfer-fee, permanent-delegate,
    // non-transferable, confidential-transfer, confidential-transfer-fee.
}

// checked_* everywhere
require!(amount.checked_add(fee).is_some(), ErrorCode::Overflow);
let pay = amount.checked_sub(fee).ok_or(ErrorCode::Overflow)?;

// emit typed events for indexers
emit!(TreasuryTransfer {
    from: vault.key(),
    to: destination.key(),
    amount,
    ts: clock.unix_timestamp,
});
```

### Compliance / jurisdictional

- Token launch jurisdiction check (US, EU, Asia-Pacific vary on securities classification).
- Off-chain OFAC screening at frontend / indexer layer.
- Helius / Triton private RPC with MEV protection for treasury ops.
- Audit (Neodyme / OtterSec / Trail of Bits) before mainnet.
- Bug bounty public after audit.

### Verification checklist before launch

1. Mint authority revoked or transferred to PDA.
2. Freeze authority revoked or transferred to PDA.
3. Treasury PDA canonical bump stored + reused in CPI signer seeds.
4. Squads multisig m ≥ 2 controls all admin ops.
5. All Token-2022 hostile extensions rejected at vault init.
6. `has_one = mint` + `owner == vault_pda` constraints on every CPI.
7. `checked_*` arithmetic on every u64/u128 op.
8. Typed events emitted for every state transition.
9. Program upgrade authority revoked or Squads-controlled with timelock.
10. Audit complete + bug bounty live.
11. Off-chain indexer contract documented (PostgreSQL/Timescale schema for events).