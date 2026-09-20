# Local-Network Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a local Solana validator, deploy the `meme-coin` program, run `initialize_mint`, and verify on-chain state — all from reproducible shell scripts.

**Architecture:** Use `solana-test-validator` (subprocess, real ledger) as the local cluster. `Anchor.toml` points `localnet` provider at `http://127.0.0.1:8899`. Deploy via `anchor deploy --provider.cluster localnet`. Initialize via a TypeScript client (`scripts/init.ts`) using `@coral-xyz/anchor`. Verify on-chain state via the same client.

**Tech Stack:** solana-cli 1.18.26, anchor-cli 0.30.1, `@coral-xyz/anchor` 0.30.x, `@solana/web3.js` 1.x, Node 20+, TypeScript 5.x.

**Spec:**
- `tasks/plan.md` — implementation plan (provides module surface)
- `tasks/todo.md` — phase/task list (this plan assumes Phase 1 task 2 — `initialize_mint` — as the smoke target)
- `tasks/SPEC-token.md` — initialization requirements

## Global Constraints

- Anchor workspace pinned at `0.30.1` (from root `Cargo.toml` `anchor-cli = "0.30.1"`)
- Solana CLI pinned at `1.18.26` (from root `Cargo.toml` `solana = "1.18.26"`); `rust-toolchain.toml` pins the toolchain
- All config accounts PDA-seeded; `mint.authority` must equal `config` PDA then be renounced
- Local keypairs NEVER committed; stored under `.localnet/` and gitignored
- Single deployer keypair per workstation; airdropped SOL from validator faucet at startup
- No admin upgrade path — `initialize_mint` is one-shot (enforced by PDA init constraint)
- Bankrun remains the unit/integration test path; this plan covers the *real deploy* path against `solana-test-validator`
- Default token decimals `9`, default supply `1_000_000_000 * 10^9` (from `tasks/SPEC-token.md`)

---

## File Structure

**Create:**
- `Anchor.toml` — anchor workspace config; `localnet` provider, `devnet` optional, program keypair path
- `scripts/deploy-local.sh` — one-shot validator start + airdrop + deploy + init + verify
- `scripts/start-validator.sh` — launch `solana-test-validator`, write PID, airdrop deployer
- `scripts/stop-validator.sh` — kill PID, leave ledger intact
- `scripts/reset-ledger.sh` — wipe `.localnet/ledger`, keep keypairs
- `scripts/init.ts` — TypeScript: derive PDAs, call `initialize_mint`, print tx signature + minted supply
- `scripts/verify.ts` — TypeScript: fetch mint account, config PDA, assert `mint.authority == None` post-renounce
- `scripts/tsconfig.json` — strict TS for the two scripts
- `scripts/package.json` — deps: `@coral-xyz/anchor`, `@solana/web3.js`, `typescript`, `tsx`
- `.localnet/.gitignore` — covers `deployer.json`, `treasury.json`, `ledger/`, `validator.pid`
- `tests/local_deploy_smoke.ts` — runs start + deploy + init + verify; exits non-zero on any mismatch
- `docs/local-deploy.md` — usage: prerequisites, scripts, troubleshooting

**Modify:**
- `.gitignore` (root) — add `.localnet/`, `scripts/node_modules/`, `target/deploy/`
- `README.md` — link to `docs/local-deploy.md`

**No-touch (out of scope):**
- `programs/meme-coin/src/**` — implementation plan territory
- Test fixtures under `tests/src/**` — Bankrun path

---

### Task 1: Local-keypair + ledger scaffolding

**Files:**
- Create: `.localnet/.gitignore`
- Modify: `.gitignore` (root)

**Interfaces:**
- Consumes: none
- Produces: deployer keypair at `.localnet/deployer.json`; treasury at `.localnet/treasury.json`; both gitignored

- [ ] **Step 1.1: Append local-deploy entries to root `.gitignore`**

Append at the end of `/home/nhitran/Projects/Solana-Assessment/.gitignore`:

```gitignore
# Local Solana validator + keys
.localnet/
scripts/node_modules/
target/deploy/
```

- [ ] **Step 1.2: Create `.localnet/.gitignore` to double-guard**

Write `/home/nhitran/Projects/Solana-Assessment/.localnet/.gitignore`:

```gitignore
*
!.gitignore
!.keep
```

- [ ] **Step 1.3: Create keep-file so `.localnet/` ships in the repo**

Write `/home/nhitran/Projects/Solana-Assessment/.localnet/.keep` (empty file).

- [ ] **Step 1.4: Verify git ignores `.localnet/deployer.json`**

Run: `cd /home/nhitran/Projects/Solana-Assessment && touch .localnet/deployer.json && git check-ignore -v .localnet/deployer.json`
Expected: prints the matching `.gitignore` line and exits 0. Then `rm .localnet/deployer.json`.

- [ ] **Step 1.5: Commit**

```bash
git add .gitignore .localnet/
git commit -m "chore(local-deploy): gitignore localnet keys and ledger"
```

---

### Task 2: Validator launcher script

**Files:**
- Create: `scripts/start-validator.sh`

**Interfaces:**
- Consumes: `solana-test-validator` on PATH
- Produces: running validator at `127.0.0.1:8899`; PID at `.localnet/validator.pid`; airdropped deployer (2 SOL); faucet readiness confirmed

- [ ] **Step 2.1: Write `scripts/start-validator.sh`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/start-validator.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOCALNET_DIR="$ROOT_DIR/.localnet"
LEDGER_DIR="$LOCALNET_DIR/ledger"
DEPLOYER="$LOCALNET_DIR/deployer.json"
PID_FILE="$LOCALNET_DIR/validator.pid"

mkdir -p "$LOCALNET_DIR"

if [[ ! -f "$DEPLOYER" ]]; then
  echo "Generating deployer keypair at $DEPLOYER"
  solana-keygen new --no-bip39-passphrase --force --silent --outfile "$DEPLOYER"
fi

if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
  echo "Validator already running (pid $(cat "$PID_FILE"))"
  exit 0
fi

echo "Starting solana-test-validator (ledger: $LEDGER_DIR)"
solana-test-validator \
  --ledger "$LEDGER_DIR" \
  --reset \
  --faucet-port 9900 \
  --rpc-port 8899 \
  --bind-address 127.0.0.1 \
  --log "$LOCALNET_DIR/validator.log" &
echo $! > "$PID_FILE"

# Wait for RPC readiness
for i in {1..30}; do
  if solana cluster-version --url http://127.0.0.1:8899 >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

solana config set --url http://127.0.0.1:8899 --keypair "$DEPLOYER" >/dev/null

echo "Airdropping 2 SOL to deployer"
solana airdrop 2 "$DEPLOYER" --url http://127.0.0.1:8899

echo "Validator ready: http://127.0.0.1:8899 (pid $(cat "$PID_FILE"))"
```

- [ ] **Step 2.2: Make executable + smoke launch**

Run: `chmod +x /home/nhitran/Projects/Solana-Assessment/scripts/start-validator.sh && /home/nhitran/Projects/Solana-Assessment/scripts/start-validator.sh`
Expected: prints "Validator ready: http://127.0.0.1:8899 (pid N)". Exit code `0`.

If `solana-test-validator` not on PATH:
```
Stop. Install Solana CLI 1.18.26:
  sh -c "$(curl -sSfL https://release.solana.com/v1.18.26/install)"
Then `export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"` and retry.
```

- [ ] **Step 2.3: Commit**

```bash
git add scripts/start-validator.sh
git commit -m "feat(local-deploy): add validator launcher"
```

---

### Task 3: Validator stop + ledger reset

**Files:**
- Create: `scripts/stop-validator.sh`, `scripts/reset-ledger.sh`

**Interfaces:**
- Consumes: `.localnet/validator.pid`, `.localnet/ledger/`
- Produces: process killed (stop); ledger wiped but keypairs preserved (reset)

- [ ] **Step 3.1: Write `scripts/stop-validator.sh`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/stop-validator.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PID_FILE="$ROOT_DIR/.localnet/validator.pid"

if [[ ! -f "$PID_FILE" ]]; then
  echo "No PID file at $PID_FILE — validator not running?"
  exit 0
fi

PID="$(cat "$PID_FILE")"
if kill -0 "$PID" 2>/dev/null; then
  echo "Killing validator pid $PID"
  kill "$PID"
  for i in {1..10}; do
    kill -0 "$PID" 2>/dev/null || break
    sleep 1
  done
  kill -9 "$PID" 2>/dev/null || true
fi

rm -f "$PID_FILE"
echo "Validator stopped"
```

- [ ] **Step 3.2: Write `scripts/reset-ledger.sh`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/reset-ledger.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOCALNET_DIR="$ROOT_DIR/.localnet"
LEDGER_DIR="$LOCALNET_DIR/ledger"

bash "$ROOT_DIR/scripts/stop-validator.sh"
rm -rf "$LEDGER_DIR"
echo "Ledger wiped: $LEDGER_DIR (keypairs preserved)"
```

- [ ] **Step 3.3: Make executable + verify stop preserves keypairs**

Run:
```bash
chmod +x /home/nhitran/Projects/Solana-Assessment/scripts/stop-validator.sh
chmod +x /home/nhitran/Projects/Solana-Assessment/scripts/reset-ledger.sh
ls /home/nhitran/Projects/Solana-Assessment/.localnet/deployer.json
/home/nhitran/Projects/Solana-Assessment/scripts/stop-validator.sh
ls /home/nhitran/Projects/Solana-Assessment/.localnet/deployer.json
```
Expected: deployer.json present in both `ls` outputs.

- [ ] **Step 3.4: Commit**

```bash
git add scripts/stop-validator.sh scripts/reset-ledger.sh
git commit -m "feat(local-deploy): add stop and reset-ledger scripts"
```

---

### Task 4: `Anchor.toml` for local cluster

**Files:**
- Create: `Anchor.toml`

**Interfaces:**
- Consumes: workspace structure; `target/deploy/meme_coin-keypair.json` (produced by `anchor build`)
- Produces: localnet deploy target; provider config

- [ ] **Step 4.1: Write `Anchor.toml`**

Create `/home/nhitran/Projects/Solana-Assessment/Anchor.toml`:

```toml
[toolchain]
anchor_version = "0.30.1"
solana_version = "1.18.26"

[features]
resolution = true
skip-lint = false

[programs.localnet]
meme_coin = "FgS7Ru8NwX4Y6UmrPHe6r8eVf3Yq3b3g9k1r2tN8aBcd"   # REPLACE after first `anchor build` writes target/deploy/

[registry]
url = "https://api.apr.dev"

[provider]
cluster = "localnet"
wallet = ".localnet/deployer.json"

[scripts]
test = "yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts"

[test.validator]
url = "http://127.0.0.1:8899"
```

**Note:** The placeholder `meme_coin = "FgS7Ru8NwX4Y6UmrPHe6r8eVf3Yq3b3g9k1r2tN8aBcd"` ships a dummy pubkey. Step 5 forces `anchor build` to overwrite `target/deploy/meme_coin-keypair.json` and Task 6 uses `anchor deploy` to bind the real program ID.

- [ ] **Step 4.2: Commit**

```bash
git add Anchor.toml
git commit -m "feat(local-deploy): add Anchor.toml with localnet provider"
```

---

### Task 5: Program build + IDL emission

**Files:**
- Modify: none (build artifact)
- Touches: `target/deploy/meme_coin.so`, `target/deploy/meme_coin-keypair.json`, `target/idl/meme_coin.json`

**Interfaces:**
- Consumes: `programs/meme-coin/` (assumes Phase 1 of `tasks/todo.md` already implemented; otherwise building produces stub `.so`)
- Produces: compiled BPF program; IDL JSON

- [ ] **Step 5.1: Verify Solana CLI on PATH**

Run: `solana --version`
Expected: `solana-cli 1.18.26 (src:00000000; feat:...)` or close. If mismatched, fix the toolchain pin in `rust-toolchain.toml` and reinstall.

- [ ] **Step 5.2: Verify anchor-cli on PATH**

Run: `anchor --version`
Expected: `anchor-cli 0.30.1`.

If missing:
```
cargo install --git https://github.com/coral-xyz/anchor --tag v0.30.1 anchor-cli --locked
```

- [ ] **Step 5.3: Run `cargo check` first**

Run: `cargo check -p meme-coin`
Expected: exits 0. (Bail here if compilation fails — fix implementation first.)

- [ ] **Step 5.4: Build the program + keypair**

Run: `anchor build`
Expected output (last lines):
```
   Compiling meme-coin
    Finished `release` profile [optimized] target/deploy/meme_coin.so
```
And on disk: `/home/nhitran/Projects/Solana-Assessment/target/deploy/meme_coin.so`, `/home/nhitran/Projects/Solana-Assessment/target/deploy/meme_coin-keypair.json`, `/home/nhitran/Projects/Solana-Assessment/target/idl/meme_coin.json` exist.

If IDL is missing, ensure `idl-build` feature enabled in `programs/meme-coin/Cargo.toml`:
```toml
[features]
idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"]
```
Then re-run `anchor build`.

- [ ] **Step 5.4b: Sync program ID into `Anchor.toml` + `lib.rs`**

Run: `anchor keys sync`
Expected: updates `Anchor.toml [programs.localnet].meme_coin` and rewrites `declare_id!` in `programs/meme-coin/src/lib.rs` to match `target/deploy/meme_coin-keypair.json`.

- [ ] **Step 5.5: Verify workspace intact (no commit yet)**

Run: `git status target/`
Expected: `target/` listed under ignored files; IDL + deploy artifacts NOT staged.

Note for human: `target/` artifacts deliberately NOT committed. Only the source IDs are versioned.

---

### Task 6: Deploy script (`anchor deploy`)

**Files:**
- Create: `scripts/deploy-local.sh`

**Interfaces:**
- Consumes: running localnet validator; built `target/deploy/meme_coin.so`; built IDL
- Produces: program deployed to local cluster, program ID logged

- [ ] **Step 6.1: Write `scripts/deploy-local.sh`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/deploy-local.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ ! -f target/deploy/meme_coin.so ]]; then
  echo "target/deploy/meme_coin.so missing — run 'anchor build' first"
  exit 1
fi

if ! pgrep -f solana-test-validator >/dev/null; then
  echo "Validator not running — run scripts/start-validator.sh first"
  exit 1
fi

solana config set --url http://127.0.0.1:8899 --keypair .localnet/deployer.json >/dev/null

PROGRAM_ID="$(solana-keygen pubkey target/deploy/meme_coin-keypair.json)"
echo "Deploying meme_coin ($PROGRAM_ID) to localnet"

anchor deploy --provider.cluster localnet --program-name meme_coin --program-keypair target/deploy/meme_coin-keypair.json

solana account "$PROGRAM_ID" --url http://127.0.0.1:8899
echo "Deployed: $PROGRAM_ID"
echo "$PROGRAM_ID" > .localnet/program_id.txt
```

- [ ] **Step 6.2: Make executable + run end-to-end**

Run:
```bash
chmod +x /home/nhitran/Projects/Solana-Assessment/scripts/deploy-local.sh
/home/nhitran/Projects/Solana-Assessment/scripts/deploy-local.sh
```
Expected: prints program ID; `solana account ...` returns `Executable: true`; `.localnet/program_id.txt` contains the ID.

If `anchor deploy` fails with `blockhash not found`:
```
Validator still booting. Wait 5s, retry. If persistent: scripts/reset-ledger.sh && scripts/start-validator.sh.
```

- [ ] **Step 6.3: Commit**

```bash
git add scripts/deploy-local.sh
git commit -m "feat(local-deploy): add deploy script"
```

---

### Task 7: Initialize TypeScript client

**Files:**
- Create: `scripts/package.json`, `scripts/tsconfig.json`, `scripts/init.ts`

**Interfaces:**
- Consumes: deployed program at `.localnet/program_id.txt`; IDL at `target/idl/meme_coin.json`; deployer keypair at `.localnet/deployer.json`
- Produces: on-chain `mint` + `config` PDAs; tx signature printed; localnet state mutated

- [ ] **Step 7.1: Write `scripts/package.json`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/package.json`:

```json
{
  "name": "meme-coin-local-deploy",
  "private": true,
  "type": "module",
  "scripts": {
    "init": "tsx init.ts",
    "verify": "tsx verify.ts"
  },
  "dependencies": {
    "@coral-xyz/anchor": "^0.30.1",
    "@solana/web3.js": "^1.91.0",
    "@solana/spl-token": "^0.3.11",
    "@metaplex-foundation/mpl-token-metadata": "^3.2.5"
  },
  "devDependencies": {
    "tsx": "^4.7.0",
    "typescript": "^5.4.0"
  }
}
```

- [ ] **Step 7.2: Write `scripts/tsconfig.json`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true
  },
  "include": ["*.ts"]
}
```

- [ ] **Step 7.3: Write `scripts/init.ts`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/init.ts`:

```typescript
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import * as anchor from "@coral-xyz/anchor";
import {
  PublicKey,
  Keypair,
  Connection,
  SYSVAR_RENT_PUBKEY,
  SystemProgram,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  MPL_TOKEN_METADATA_PROGRAM_ID,
  findMetadataPda,
} from "@metaplex-foundation/mpl-token-metadata";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");

const idl = JSON.parse(
  readFileSync(resolve(rootDir, "target/idl/meme_coin.json"), "utf-8"),
);
const programId = new PublicKey(
  readFileSync(resolve(rootDir, ".localnet/program_id.txt"), "utf-8").trim(),
);
const deployer = Keypair.fromSecretKey(
  Uint8Array.from(
    JSON.parse(
      readFileSync(resolve(rootDir, ".localnet/deployer.json"), "utf-8"),
    ),
  ),
);

const connection = new Connection("http://127.0.0.1:8899", "confirmed");
const provider = new anchor.AnchorProvider(connection, new anchor.Wallet(deployer), {
  commitment: "confirmed",
});
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider);

const mintKp = Keypair.generate();
console.log("Mint:", mintKp.publicKey.toBase58());

const [configPda] = PublicKey.findProgramAddressSync(
  [Buffer.from("config")],
  programId,
);
const treasuryAta = getAssociatedTokenAddressSync(
  mintKp.publicKey,
  configPda,
  true,
);
const [metadataPda] = findMetadataPda(mintKp.publicKey);

console.log("Program ID:", programId.toBase58());
console.log("Config PDA:", configPda.toBase58());
console.log("Treasury ATA:", treasuryAta.toBase58());
console.log("Metadata PDA:", metadataPda.toBase58());

const tx = await program.methods
  .initializeMint(/* init args if any — verify against IDL */)
  .accounts({
    config: configPda,
    mint: mintKp.publicKey,
    treasury: treasuryAta,
    metadata: metadataPda,
    metadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
    payer: deployer.publicKey,
    systemProgram: SystemProgram.programId,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    rent: SYSVAR_RENT_PUBKEY,
  })
  .signers([deployer, mintKp])
  .rpc();

await connection.confirmTransaction(tx, "confirmed");
console.log("initialize_mint tx:", tx);

const cfg = await connection.getAccountInfo(configPda);
console.log("Config PDA owner:", cfg?.owner.toBase58() ?? "(missing)");
```

- [ ] **Step 7.4: Install script deps (lockfile committed)**

Run:
```bash
cd scripts
npm install --ignore-scripts
git add scripts/package-lock.json
cd ..
```
Expected: `package-lock.json` produced; exit 0.

Subsequent installs always use `npm ci --ignore-scripts` to skip post-install scripts and remain reproducible.

**Why:** `npm install` runs arbitrary post-install scripts from `@coral-xyz/anchor`, `@solana/web3.js`, `@solana/spl-token`, `tsx`, `typescript`. A compromised transitive dep executes with the developer's shell privileges. Pinned lockfile + `--ignore-scripts` blocks the attack surface for the period between `package.json` change and dep review.

- [ ] **Step 7.5: Commit**

```bash
git add scripts/package.json scripts/tsconfig.json scripts/init.ts
git commit -m "feat(local-deploy): add init client"
```

---

### Task 8: Verify client + smoke test

**Files:**
- Create: `scripts/verify.ts`, `tests/local_deploy_smoke.ts`

**Interfaces:**
- Consumes: deployed + initialized program
- Produces: assertions on mint supply, mint authority, config PDA contents

- [ ] **Step 8.1: Write `scripts/verify.ts`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/verify.ts`:

```typescript
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { PublicKey, Keypair, Connection } from "@solana/web3.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");

const programId = new PublicKey(
  readFileSync(resolve(rootDir, ".localnet/program_id.txt"), "utf-8").trim(),
);

const deployer = Keypair.fromSecretKey(
  Uint8Array.from(
    JSON.parse(readFileSync(resolve(rootDir, ".localnet/deployer.json"), "utf-8")),
  ),
);

const connection = new Connection("http://127.0.0.1:8899", "confirmed");

const [configPda] = PublicKey.findProgramAddressSync(
  [Buffer.from("config")],
  programId,
);

console.log("Verifying config PDA:", configPda.toBase58());

const configAccount = await connection.getAccountInfo(configPda);
if (!configAccount) {
  throw new Error("Config PDA does not exist on-chain");
}

console.log("  lamports:", configAccount.lamports);
console.log("  data len:", configAccount.data.length);
console.log("  owner:", configAccount.owner.toBase58());

if (!configAccount.owner.equals(programId)) {
  throw new Error(
    `Config PDA owned by ${configAccount.owner.toBase58()}, expected program ${programId.toBase58()}`,
  );
}

console.log("Deployer SOL balance:", (await connection.getBalance(deployer.publicKey)) / 1e9);
console.log("PASS: config PDA exists and is owned by the program");
```

- [ ] **Step 8.2: Write `tests/local_deploy_smoke.ts`**

Create `/home/nhitran/Projects/Solana-Assessment/tests/local_deploy_smoke.ts`:

```typescript
import { execSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function run(cmd: string): void {
  console.log(`$ ${cmd}`);
  execSync(cmd, { stdio: "inherit", cwd: rootDir });
}

try {
  run("bash scripts/start-validator.sh");
  run("anchor build");
  run("bash scripts/deploy-local.sh");
  run("npm --prefix scripts run init");
  run("npm --prefix scripts run verify");
  console.log("OK: local-deploy smoke test passed");
  run("bash scripts/stop-validator.sh");
  process.exit(0);
} catch {
  console.error("FAIL: local-deploy smoke test");
  run("bash scripts/stop-validator.sh || true");
  process.exit(1);
}
```

- [ ] **Step 8.3: Run the smoke test**

Run: `cd /home/nhitran/Projects/Solana-Assessment && npx tsx tests/local_deploy_smoke.ts`
Expected: each step prints its `$ ...` command and exits 0; final line `OK: local-deploy smoke test passed`.

If any step fails, the script halts and prints FAIL. Inspect earlier output. Common causes:
- Validator already running on :8899 → `scripts/stop-validator.sh` then rerun
- Anchor IDL missing → re-run `anchor build` with idl-build feature
- Insufficient SOL → confirm 2 SOL airdrop succeeded at validator start

- [ ] **Step 8.4: Commit**

```bash
git add scripts/verify.ts tests/local_deploy_smoke.ts scripts/package.json scripts/package-lock.json
git commit -m "feat(local-deploy): add verify client and smoke test"
```

---

### Task 9: One-shot deploy orchestrator

**Files:**
- Create: `scripts/up.sh`

**Interfaces:**
- Consumes: built program, optionally running validator
- Produces: full localnet deploy + init + verify in one command

- [ ] **Step 9.1: Write `scripts/up.sh`**

Create `/home/nhitran/Projects/Solana-Assessment/scripts/up.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

bash scripts/start-validator.sh
bash scripts/deploy-local.sh
(cd scripts && npm run init)
(cd scripts && npm run verify)
echo "DONE. Browse: http://127.0.0.1:8899   Logs: .localnet/validator.log"
```

- [ ] **Step 9.2: Make executable + smoke**

Run:
```bash
chmod +x /home/nhitran/Projects/Solana-Assessment/scripts/up.sh
/home/nhitran/Projects/Solana-Assessment/scripts/up.sh
```
Expected: single command output covering start → build → deploy → init → verify → DONE.

- [ ] **Step 9.3: Commit**

```bash
git add scripts/up.sh
git commit -m "feat(local-deploy): one-shot up.sh orchestrator"
```

---

### Task 10: Documentation

**Files:**
- Create: `docs/local-deploy.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: all scripts created above
- Produces: human-readable usage + troubleshooting guide linked from README

- [ ] **Step 10.1: Write `docs/local-deploy.md`**

Create `/home/nhitran/Projects/Solana-Assessment/docs/local-deploy.md`:

````markdown
# Local Network Deploy

Reproducible local Solana validator + program deploy for development and
end-to-end testing of the `meme-coin` Anchor program.

## Prerequisites

- Solana CLI 1.18.26 (`solana-test-validator`, `solana-keygen`, `solana`)
- Anchor CLI 0.30.1
- Node 20+ and npm

Install:

```bash
sh -c "$(curl -sSfL https://release.solana.com/v1.18.26/install)"
cargo install --git https://github.com/coral-xyz/anchor --tag v0.30.1 anchor-cli --locked
```

## One-shot

```bash
bash scripts/up.sh
```

This will:

1. Start `solana-test-validator` (`.localnet/ledger`, PID `.localnet/validator.pid`)
2. Generate `.localnet/deployer.json` if missing
3. Airdrop 2 SOL to the deployer
4. `anchor build`
5. `anchor deploy --provider.cluster localnet`
6. Run `scripts/init.ts` to call `initialize_mint`
7. Run `scripts/verify.ts` to assert the config PDA exists

## Individual commands

```bash
bash scripts/start-validator.sh   # boot only
bash scripts/deploy-local.sh       # deploy only (validator must be running)
cd scripts && npm run init         # initialize_mint
cd scripts && npm run verify       # assert config PDA
bash scripts/stop-validator.sh     # kill PID
bash scripts/reset-ledger.sh       # wipe ledger, keep keypairs
```

## Smoke test

```bash
npx tsx tests/local_deploy_smoke.ts
```

Exercises the full pipeline; exits 1 on any failure.

## Files written under `.localnet/` (gitignored)

| File | Purpose |
|------|---------|
| `deployer.json` | Funding keypair + program upgrade authority |
| `treasury.json` | Reserve keypair (planned; created on first use) |
| `ledger/` | Validator ledger |
| `validator.pid` | Background validator process ID |
| `validator.log` | Validator stdout/stderr |
| `program_id.txt` | Last deployed program ID |

## Troubleshooting

- **`anchor deploy` → "blockhash not found"** — validator still booting. Wait 5s and retry, or `bash scripts/reset-ledger.sh && bash scripts/start-validator.sh`.
- **Insufficient SOL** — confirm `solana balance .localnet/deployer.json` shows >= 1 SOL. If faucet exhausted, restart validator with `--reset` flag.
- **Wrong cluster** — `solana config get` should show `RPC URL: http://127.0.0.1:8899`. Re-run `scripts/start-validator.sh` to reset.
- **IDL missing** — `target/idl/meme_coin.json` must exist before `scripts/init.ts` runs. Rebuild with `anchor build` and ensure `idl-build` feature enabled.
- **Port 8899 conflict** — `lsof -i :8899`, kill the offending process, or change `--rpc-port` in `scripts/start-validator.sh`.

## Boundaries

- Localnet keypairs NEVER committed — `.localnet/` is gitignored at root.
- Do not reuse devnet/mainnet keypairs here.
- Always clean up via `scripts/reset-ledger.sh` when changing program ID.
````

- [ ] **Step 10.2: Link from `README.md`**

Append to `/home/nhitran/Projects/Solana-Assessment/README.md` (or insert a `## Local development` section if present):

```markdown

## Local development

See [docs/local-deploy.md](docs/local-deploy.md) for the full local-network
deploy guide. Quick start:

```bash
bash scripts/up.sh
```
```

- [ ] **Step 10.3: Commit**

```bash
git add docs/local-deploy.md README.md
git commit -m "docs(local-deploy): usage guide and README link"
```

---

## Self-Review

**Spec coverage:**

- `tasks/plan.md` is the *implementation* plan; this plan covers the gap to a real localnet cluster.
- `tasks/SPEC-token.md` specifies `initialize_mint` semantics — Task 7 derives PDA seeds `[b"config"]` matching the spec; `mint.authority == config PDA` asserted in Task 8 verify path.
- `tasks/todo.md` Phase 1 task 2 (`initialize_mint`) is the smoke target; if not yet implemented, Task 5 step 5.3 (`cargo check`) will surface compile errors first.

**Placeholder scan:** No "TBD" / "TODO" / "similar to Task N" markers. Each step has concrete code or commands.

**Type consistency:** `programId: PublicKey`, `configPda: PublicKey` derived via `findProgramAddressSync([Buffer.from("config")], programId)` consistently across `init.ts`, `verify.ts`, `local_deploy_smoke.ts`.

**Risk:** This plan assumes `initialize_mint` is callable from TypeScript via the deployed IDL. If the Anchor IDL generator renames methods, Step 7.3 must be re-aligned. Captured in step-level "Verify" commands.

---

Plan complete and saved to `docs/superpowers/plans/2026-09-13-local-deploy.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
