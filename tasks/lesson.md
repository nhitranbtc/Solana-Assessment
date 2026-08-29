# Project Lessons Ledger

Local corrections + project rules learned from work in this repo. Append-only.
Read at session start. Iterate until mistake rate drops.

Format per entry:
```
### L<N> | <YYYY-MM-DD>
- trigger: what surfaced the lesson
- rule: the rule itself
- why: consequence of ignoring
- apply: when/how to use this
```

---

### L1 | 2026-08-29
- trigger: scaffold state/events/errors modules before any instruction body
- rule: new program module = namespace + type scaffolding first; instruction logic lands in later commits
- why: keeps PRs atomic and reviewable; downstream account layouts/indexers/event subscribers can build against committed types without rebase churn
- apply: when adding a new module to `programs/meme-coin/src/...`, land the `mod.rs` + state struct / event variants / error enum first, then instructions

### L2 | 2026-08-29
- trigger: `errors.rs` ships one `ErrorCode` enum with `module` + downstream variants
- rule: centralize all program errors in a single enum; downstream modules add their variants, do not spawn parallel enums
- why: anchor `#[error_code]` requires single enum per program; split enums force callers to handle multiple error types and break uniform client decoding
- apply: when introducing a new failure mode, add a variant to `ErrorCode` with module prefix; never `pub enum XxxError`

### L3 | 2026-08-29
- trigger: all event types tagged `#[index]` for indexer lookup
- rule: every `#[event]` struct field that an indexer needs to filter on MUST carry `#[index]`
- why: indexers cannot reconstruct query plans from raw event bytes without indexed discriminators; missing index = unfilterable scan
- apply: when defining a new event, mark every pubkey/mint/authority/role field with `#[index]`; strings/amounts stay unindexed unless query-by-value is required

### L4 | 2026-08-29
- trigger: toolchain pinned (rust 1.79.0, anchor-cli 0.30.1, node >=20) in workspace + program manifests
- rule: toolchain pins are committed; never bump in feature commit
- why: silent toolchain drift produces "works on my machine" build failures and invalidates IDL generated against the pinned `anchor-cli`
- apply: bump toolchain in dedicated `chore:` commit; update `rust-toolchain.toml`, all `Cargo.toml` `anchor-lang` versions, and `package.json` engines in the same commit

---

## New Entries

Add below. Increment `L<N>`. Date = today.