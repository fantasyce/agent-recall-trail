# ART Knowledge Backup and Disaster Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic, Git-safe ART Knowledge Vault backups, encrypted local-authority recovery, and atomic empty-home restoration, then prove the current 28 Editions recover from a fresh private Git clone.

**Architecture:** `art-knowledge` owns the canonical snapshot manifest, allowlisted immutable-file verification, and staged restore. `art-cli` exposes operator-only backup commands. Two explicit shell workflows combine those primitives with Git, SQLite's consistent backup operation, and `age` encryption without changing ART publication or the six-tool MCP surface.

**Tech Stack:** Rust 2024, rusqlite, Serde, SHA-256, Clap, Bash, Git, SQLite CLI, age CLI, GitHub CLI.

**Spec:** `docs/superpowers/specs/2026-08-30-art-knowledge-backup-dr-design.md`

## Global Constraints

- ART remains per-Agent private memory plus human-reviewed immutable shared Knowledge Editions.
- Only `editions/**/*.md`, `editions/**/*.json`, and `.art/events/*.json` enter the canonical knowledge snapshot.
- Agent Vaults, raw queries, Recall Bundles, SQLite/WAL/SHM files, configuration, logs, and plaintext keys never enter Git.
- Backup/restore remain operator-only CLI/script surfaces; the MCP surface remains exactly six tools.
- ART publication never stages, commits, pushes, switches branches, merges, rebases, or configures a remote.
- Restore requires an absent target home and publishes it only after full verification and projection rebuild.
- All production behavior is introduced with an observed RED test before implementation.
- Release target is `0.1.1`; formal data remains under `~/.across`.

---

### Task 1: Deterministic knowledge snapshot and verification

**Files:**
- Create: `crates/art-knowledge/src/backup.rs`
- Modify: `crates/art-knowledge/src/lib.rs`
- Create: `crates/art-knowledge/tests/backup_contracts.rs`

**Interfaces:**
- Produces: `BackupManifest`, `BackupFile`, `create_backup(source, target, generator)`, and `verify_backup(root)`.
- `BackupManifest.schema` is exactly `art.knowledge.backup.v1` and `tree_sha256` is the canonical hash of the sorted file records excluding the manifest itself.

- [ ] **Step 1: Write failing deterministic-manifest tests**

Create two equivalent published Knowledge Vault fixtures and assert that two calls to:

```rust
let manifest = create_backup(source.root(), &output, "art 0.1.1")?;
```

produce byte-identical `art-backup.json`, sorted relative paths under `knowledge/editions` and `knowledge/events`, and matching Edition/event counts.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p art-knowledge --test backup_contracts deterministic_backup_is_path_and_time_independent -- --exact`

Expected: compilation fails because `art_knowledge::backup` and `create_backup` do not exist.

- [ ] **Step 3: Implement the minimal snapshot types and writer**

Use these public signatures:

```rust
pub struct BackupFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

pub struct BackupManifest {
    pub schema: String,
    pub generator: String,
    pub edition_count: u64,
    pub event_count: u64,
    pub tree_sha256: String,
    pub files: Vec<BackupFile>,
}

pub fn create_backup(source: &Path, target: &Path, generator: &str)
    -> ArtResult<BackupManifest>;
pub fn verify_backup(root: &Path) -> ArtResult<BackupManifest>;
```

Walk only `editions` and `.art/events`, reject links and non-regular files,
translate into the stable backup layout, sort records by UTF-8 relative path,
and write `art-backup.json` last with owner-only permissions.

- [ ] **Step 4: Add failing corruption and inventory tests**

Assert `verify_backup` rejects a modified Markdown byte, missing manifest,
extra file, duplicate manifest record, unsupported path, symlink, hard link,
FIFO, unsorted inventory, invalid count, and tree-hash mismatch.

- [ ] **Step 5: Run the negative tests and verify RED**

Run: `cargo test -p art-knowledge --test backup_contracts verify_backup_ -- --nocapture`

Expected: assertions fail because verification currently checks only the happy path.

- [ ] **Step 6: Implement strict verification and refactor immutable-file checks**

Extract reusable Edition/event parsing and hash validation from projection
rebuild. Compare the exact on-disk inventory with the manifest, reject all
non-allowlisted types and paths, and validate each Edition Markdown/JSON pair
and lifecycle event before returning the manifest.

- [ ] **Step 7: Run focused and crate tests GREEN**

Run: `cargo test -p art-knowledge --all-features`

Expected: all `art-knowledge` tests pass with no ignored backup tests.

- [ ] **Step 8: Commit**

```bash
git add crates/art-knowledge
git commit -m "feat: create deterministic ART knowledge backups"
```

### Task 2: Atomic restore and operator CLI

**Files:**
- Modify: `crates/art-knowledge/src/backup.rs`
- Modify: `crates/art-cli/src/main.rs`
- Modify: `crates/art-cli/tests/cli_contracts.rs`
- Modify: `crates/art-knowledge/tests/backup_contracts.rs`

**Interfaces:**
- Produces: `restore_backup(source, target_vault, commitment_key)`.
- Adds top-level `art backup create|verify|restore` commands with `art.cli.v1` JSON output.

- [ ] **Step 1: Write failing staged-restore tests**

Assert that:

```rust
restore_backup(&backup, &target, [7_u8; 32])?;
```

creates a Vault whose diagnostics report the exact Edition/event counts,
aligned index, and verified projections. Also assert an existing target,
corrupt source, invalid key length, or injected rebuild failure leaves the
target absent and removes its sibling staging directory.

- [ ] **Step 2: Verify restore tests RED**

Run: `cargo test -p art-knowledge --test backup_contracts restore_ -- --nocapture`

Expected: compilation fails because `restore_backup` does not exist.

- [ ] **Step 3: Implement staging restore**

Use:

```rust
pub fn restore_backup(
    source: &Path,
    target_vault: &Path,
    commitment_key: [u8; 32],
) -> ArtResult<KnowledgeDiagnostics>;
```

Verify first, create a randomized sibling `*.restore-staging-*`, copy the
allowlisted tree, open `KnowledgeVault`, rebuild the projection, require clean
diagnostics and exact counts, then rename the completed directory atomically.
Use a guard that deletes only the staging path on every failure.

- [ ] **Step 4: Write failing CLI contract tests**

Extend the help snapshot and test `backup create`, `backup verify`, and
`backup restore`. `restore` must require `--confirm`, an absent target home,
and a regular 32-byte key file with owner-only permissions.

- [ ] **Step 5: Verify CLI tests RED**

Run: `cargo test -p art-cli --test cli_contracts backup_ -- --nocapture`

Expected: help and command invocations fail because `BackupCommand` is absent.

- [ ] **Step 6: Implement CLI commands**

Add:

```rust
enum BackupCommand {
    Create { output: PathBuf },
    Verify { source: PathBuf },
    Restore {
        source: PathBuf,
        target_home: PathBuf,
        commitment_key: PathBuf,
        confirm: bool,
    },
}
```

Emit only safe paths, counts, tree hash, and status. Never print key bytes,
Edition content, source locators, or recovery material.

- [ ] **Step 7: Run CLI and workspace tests GREEN**

Run: `cargo test -p art-cli --all-features && cargo test --workspace --all-features`

Expected: all regular tests pass; only the existing formal performance contract remains ignored.

- [ ] **Step 8: Commit**

```bash
git add crates/art-knowledge crates/art-cli
git commit -m "feat: restore verified ART knowledge backups"
```

### Task 3: Encrypted recovery capsule and explicit Git workflow

**Files:**
- Create: `scripts/backup_knowledge_to_git.sh`
- Create: `scripts/restore_knowledge_from_git.sh`
- Create: `tests/scripts/test_backup_recovery.sh`
- Create: `scripts/install_ci_age.sh`
- Modify: `.github/workflows/quality.yml`

**Interfaces:**
- Backup script positional contract: `ART_BIN ART_HOME GIT_WORKTREE SSH_RECIPIENT_FILE EXPECTED_REPOSITORY --confirm`.
- Restore script positional contract: `ART_BIN GIT_CLONE SSH_IDENTITY_FILE TARGET_HOME --confirm`.

- [ ] **Step 1: Write a failing shell E2E**

Create a test Vault, publish fixtures, initialize a bare Git remote and worktree,
generate a task-owned age identity, invoke the backup script, clone the bare
remote, invoke the restore script, and assert exact diagnostics. Assert Git
contains only the layout defined by the spec and no plaintext marker from the
commitment key or Control Store fixture.

- [ ] **Step 2: Verify shell E2E RED**

Run: `ART_BIN=target/debug/art bash tests/scripts/test_backup_recovery.sh`

Expected: exit 127 or missing-file failure because the scripts do not exist.

- [ ] **Step 3: Implement verified age installation for CI**

Download one pinned official age release per CI platform, verify the upstream
SHA-256 from a repository constant, extract only `age` and `age-keygen`, and
prepend the task-owned directory to `GITHUB_PATH`. No package-manager mutation
is allowed in CI.

- [ ] **Step 4: Implement the backup script**

Require all explicit parameters and `--confirm`; check tools, recipient mode,
Git merge/rebase state, private GitHub visibility when the remote is GitHub,
and exact expected repository identity. Create a `0700` temporary directory,
run `art backup create`, create a consistent SQLite `.backup`, copy the key as
`0600`, tar only those two files, encrypt with `age -R`, remove plaintext via a
trap, write the safe recovery manifest, verify the snapshot, stage only the
allowlist, commit, and stop before push.

- [ ] **Step 5: Implement the restore script**

Require all parameters and `--confirm`; verify the backup and capsule digest,
decrypt/extract into `0700` temporary storage, reject unexpected archive paths,
call `art backup restore` with the recovered key, install the recovered Control
Store only after a consistent backup check, rebuild knowledge, run Doctor, and
delete plaintext recovery data through a trap.

- [ ] **Step 6: Run shell negative and round-trip tests GREEN**

Run: `ART_BIN=target/debug/art bash tests/scripts/test_backup_recovery.sh`

Expected: all assertions pass, temporary plaintext markers are absent, and no test process remains.

- [ ] **Step 7: Commit**

```bash
git add scripts tests/scripts .github/workflows/quality.yml
git commit -m "feat: back up ART knowledge to encrypted private Git"
```

### Task 4: Operations documentation and 0.1.1 release surfaces

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, crate manifests
- Modify: `README.md`, `CHANGELOG.md`, `docs/operations.md`, `docs/architecture.md`, `docs/security-model.md`, `docs/testing.md`
- Modify: `.codex-plugin/plugin.json`, `.agents/skills/art-recall/SKILL.md`, `packaging/mcp-registry/server.json.in`
- Modify: `.github/workflows/release.yml`, `.github/workflows/publish-mcp.yml`
- Modify: `scripts/install.sh`, `scripts/test_launch_surface.sh`

**Interfaces:**
- Produces ART, plugin, release assets, and MCP Registry metadata at version `0.1.1`.

- [ ] **Step 1: Write failing version and documentation checks**

Update `tests/scripts/test_release_version.sh` and backup documentation checks
to require `0.1.1`, all three CLI backup commands, no claim of automatic sync,
and explicit separation of public Git snapshot from encrypted local authority.

- [ ] **Step 2: Verify version checks RED**

Run: `ART_BIN=target/debug/art bash tests/scripts/test_release_version.sh`

Expected: failures report current `0.1.0` values and missing backup documentation.

- [ ] **Step 3: Update all release and plugin surfaces**

Change the workspace/package/plugin/registry/release/install versions together,
document backup/restore preconditions and limitations, and add `0.1.1` release
notes. Preserve exactly six MCP tools and the no-daemon/no-auto-push position.

- [ ] **Step 4: Run focused release checks GREEN**

Run: `cargo build --locked -p art-cli && ART_BIN=target/debug/art bash tests/scripts/test_release_version.sh && ART_BIN=target/debug/art bash tests/scripts/test_plugin_surface.sh && bash scripts/test_launch_surface.sh`

Expected: all checks pass at `0.1.1`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates README.md CHANGELOG.md docs .codex-plugin .agents packaging .github scripts tests
git commit -m "release: prepare ART 0.1.1 backup and recovery"
```

### Task 5: Full local acceptance and security review

**Files:**
- Modify only when a failing acceptance test demonstrates a defect.
- Create: `docs/artifacts/acceptance-2026-08-30-v0.1.1.md`

**Interfaces:**
- Produces a source-level release candidate and bounded evidence before any formal data or GitHub mutation.

- [ ] **Step 1: Run formatting, lint, unit, integration, migration, plugin, installer, open-source, secret, and independence gates**

Run the repository's complete release gate plus `tests/scripts/test_backup_recovery.sh`.

- [ ] **Step 2: Run formal performance and stress gates**

Run the ignored 10k/5k release performance contract and the 500 graceful,
100 abnormal, 1000-query, and 8-client stress suite.

- [ ] **Step 3: Scan the final source and release archives**

Require no plaintext key, Control Store, Agent Vault, source locator, host path,
credential-like fixture, build cache, or test home in the Git tree or assets.

- [ ] **Step 4: Record acceptance evidence and commit**

```bash
git add docs/artifacts/acceptance-2026-08-30-v0.1.1.md
git commit -m "test: accept ART 0.1.1 backup and recovery"
```

### Task 6: Formal private Git backup and fresh-clone disaster drill

**Files:**
- Formal source: `~/.across/data/art/knowledge-vault`
- New private remote: `fantasyce/art-knowledge-backup`
- Task-owned clone and restore home under `/private/tmp/art-v0.1.1-backup-dr-20260830`

**Interfaces:**
- Produces one private, protected Git backup with a verified 28-Edition snapshot and encrypted recovery capsule.

- [ ] **Step 1: Resolve one matching local SSH public recipient and identity without printing their bodies**

Compare fingerprints only. Do not read, log, copy into documentation, or commit
the private key or full public-key text.

- [ ] **Step 2: Create the private repository and verify privacy before data transfer**

Create `fantasyce/art-knowledge-backup` as private with issues/wiki/projects
disabled, then verify visibility and exact owner/name before adding the remote.

- [ ] **Step 3: Run formal backup, review the staged allowlist, commit, and explicitly push**

Use the published candidate binary and explicit confirmation. Inspect every
tracked path, run secret scanning, verify the snapshot/capsule bindings, then
push `main` and enable force-push/deletion protection.

- [ ] **Step 4: Perform the clean-clone restore drill**

Clone into the task root, restore into an absent temporary ART home, and require
Doctor to report 28 current Editions, aligned index, verified hashes, and no
private Agent memory. Run representative English and Chinese knowledge recall.

- [ ] **Step 5: Test fresh Codex and DSH identities against the restored home**

Both must retrieve the shared knowledge while a private-memory reference from
the formal Codex Vault returns `ART_NOT_FOUND` in the restored identities.

### Task 7: Public release, installed-runtime cutover, and old-repository retirement

**Files:**
- Public repo: `fantasyce/agent-recall-trail`
- Installed binary: `~/.across/bin/art`
- Installed plugin: `agent-recall-trail@personal`
- Old private repo: `fantasyce/agent-knowledge-base`

**Interfaces:**
- Produces merged protected `main`, tag/release/registry `v0.1.1`, installed and E2E-verified ART, and verified deletion of the old repository.

- [ ] **Step 1: Push the feature branch, open a PR, and require macOS/Linux checks**

Do not merge until both required checks pass and the branch diff contains no
formal knowledge, encrypted capsule, key material, or machine-specific paths.

- [ ] **Step 2: Merge, tag the exact protected-main commit, and publish 0.1.1**

Verify native macOS/Linux assets, SHA256SUMS, SBOM, MCPB, provenance, Pages, and
MCP Registry version `0.1.1`.

- [ ] **Step 3: Install the exact published macOS binary and plugin**

Verify release digest/provenance first, atomically replace the installed ART
binary/plugin, and retain the verified previous version only until cutover E2E passes.

- [ ] **Step 4: Run final formal Codex and DSH E2E**

Require shared knowledge recall, private capture/recall, identity isolation,
process reconnect, Doctor, backup creation, and backup verification from the
installed bytes.

- [ ] **Step 5: Delete the old private knowledge repository**

Only after Tasks 6 and 7 Steps 1-4 pass, delete exactly
`fantasyce/agent-knowledge-base`; then verify GitHub returns not found and no
local clone/config/process/path refers to it. Do not delete the new private
backup repository or formal ART data.

- [ ] **Step 6: Clean task-owned residue and record retained state**

Remove only the task worktree, clones, restored test homes, plaintext recovery
material, release downloads, sessions, branches already merged, and temporary
archives. Re-run residue verification, disk usage, Git/worktree state, and
formal ART Doctor.
