# ART Knowledge Backup and Disaster Recovery Design

**Status:** Approved by the operator on 2026-08-30

**Target release:** ART 0.1.1

## Purpose

ART's original product contract makes human-owned Markdown, immutable Edition
and lifecycle manifests, and Git history the authority for shared knowledge.
SQLite projections are disposable. The 0.1.0 runtime writes immutable files but
does not yet provide a governed Git backup or a complete restore drill. This
change closes that gap without turning Git into Agent memory, cloud sync, or an
automatic publisher.

The feature must let an operator:

1. create a deterministic snapshot of committed shared knowledge;
2. verify the snapshot without trusting its Git host;
3. preserve the non-rebuildable local control audit and commitment key only as
   an encrypted recovery capsule;
4. restore into a new, empty ART home through a staging directory;
5. rebuild and verify the shared projection before making the restored Vault
   visible; and
6. maintain that snapshot in an explicitly configured private Git repository.

## Product-boundary review

This design preserves the frozen ART boundaries:

- Private `MemoryArtifact`, `SourceAnchor`, and `AssuranceDecision` records stay
  in physically separate per-Agent Vaults and never enter the knowledge Git
  repository.
- Shared knowledge remains a separate object family. A backup neither promotes
  memory nor bypasses proposal review and human publication.
- Only committed immutable Edition files and revocation/supersession events are
  readable canonical knowledge. SQLite FTS and current pointers are rebuilt.
- The MCP surface remains exactly six Agent-safe tools. Backup, restore, Git,
  and recovery-key operations are operator-only CLI/script surfaces.
- ART remains local-first, stdio-only, and independent of AAA and Across
  Context. There is no daemon, scheduler, cloud account, or background sync.
- Git records knowledge history; it is not a project-shared memory pool, Git
  Memory feature, source-code collector, or replacement for host truth.

## Rejected alternatives

### Put the live ART home under Git

Rejected. It risks tracking per-Agent databases, WAL files, configuration,
logs, and secrets. It also makes unrelated runtime churn look like canonical
knowledge history.

### Copy the whole ART home into a backup repository

Rejected. It violates data minimization and can leak private memory, raw source
locators, the commitment key, and transient SQLite state.

### Export only Markdown and ignore local control state

Rejected as incomplete disaster recovery. Published knowledge can be rebuilt,
but proposal source locks, review receipts, and the blinded-source commitment
key are non-rebuildable local authority. They require a separate encrypted
capsule.

## Backup layout

One private Git repository contains only the following allowlisted paths:

```text
README.md
art-backup.json
knowledge/
  editions/<knowledge-key>/<edition-number>-<edition-id>.md
  editions/<knowledge-key>/<edition-number>-<edition-id>.json
  events/<event-id>.<kind>.json
recovery/
  recovery-manifest.json
  control-and-key.tar.age
```

The repository must not contain Agent Vaults, `art-control.sqlite3`, WAL/SHM
files, configuration, logs, Recall Bundles, queries, source excerpts, plaintext
keys, credentials, recovery codes, or full SSH public-key text.

`art-backup.json` uses schema `art.knowledge.backup.v1`. It contains the ART
version, sorted file inventory, byte length and SHA-256 for every canonical
knowledge file, Edition/event counts, and a deterministic tree hash. It has no
creation timestamp or machine path, so identical knowledge creates identical
snapshot bytes.

`recovery-manifest.json` uses schema `art.recovery.capsule.v1`. It binds the
encrypted capsule SHA-256 to the knowledge tree hash and records only the SSH
recipient fingerprint, never the public key itself. The capsule contains a
consistent SQLite backup of the Control Store plus `commitment.key`; it is
encrypted before it enters the Git worktree.

## Operator interfaces

The stable operator CLI adds:

```text
art backup create --output <new-directory>
art backup verify --source <directory>
art backup restore --source <directory> --target-home <empty-directory> \
  --commitment-key <0600-key-file> --confirm
```

`create` refuses an existing target, checkpoints the source Vault, copies only
the allowlisted immutable knowledge files, verifies each Edition/event, and
writes the deterministic manifest last.

`verify` rejects missing, extra, duplicate, unsupported, symlinked, hard-linked,
or digest-mismatched files. It validates path normalization, JSON schemas,
Edition Markdown/manifest binding, event hashes, inventory order, counts, and
the tree hash. It does not require a database or commitment key.

`restore` requires a target home that does not exist. It validates the source
first, requires a 32-byte owner-only commitment key file, builds the complete
target under a sibling staging directory, reconstructs the Knowledge Vault
projection from immutable files/events, runs diagnostics, and only then
atomically renames the staged ART home into place. Any failure removes only the
task-owned staging directory and leaves the target absent.

The repository also ships two explicit operator scripts:

```text
scripts/backup_knowledge_to_git.sh
scripts/restore_knowledge_from_git.sh
```

The backup script requires explicit ART home, private Git worktree, SSH
recipient file, and confirmation. It uses the release `art` binary for the
canonical snapshot, creates a consistent SQLite backup, streams an owner-only
tar archive through `age`, writes the recovery manifest, verifies both layers,
then stages and commits only the allowlisted backup paths. It never configures
a remote or pushes implicitly. The caller performs the explicit `git push`.

The restore script verifies the Git snapshot and recovery manifest, decrypts
into a task-owned `0700` temporary directory, validates extracted paths and
modes, calls `art backup restore`, replaces the restored Control Store with the
consistent recovered copy, rebuilds the projection, runs Doctor, and removes
plaintext recovery material on success or failure.

## Git policy

- The formal repository is private and dedicated to one ART Knowledge Vault.
- `main` is protected against force pushes and deletion.
- Each verified backup is one normal commit; immutable Git history supplies the
  human review/audit trail.
- ART publication still never runs `git add`, commit, push, branch switching,
  merge, or rebase.
- Backup scripts stop on merge/rebase state, unexpected tracked paths, dirty
  changes outside the allowlist, a non-private GitHub repository, or a remote
  whose resolved repository is not the explicitly expected target.
- No scheduled/background push is introduced in 0.1.1.

## Security and failure behavior

- All new directories are owner-only while local. Public Knowledge Edition
  content remains bounded by its existing Internal/Public policy.
- Source and destination path components are canonicalized; `..`, absolute
  manifest paths, symlinks, hard links, FIFOs, sockets, and devices fail closed.
- Snapshot creation never follows links and never reads Agent Vault paths.
- Encryption uses `age` with an operator-supplied SSH recipient file. ART does
  not read an SSH private key, print a recipient body, generate a recovery code,
  or persist plaintext recovery material.
- A missing `age`, `git`, recipient, clean Git state, or confirmation is a hard
  precondition failure before repository mutation.
- Restore cannot merge into a non-empty home and cannot overwrite a working
  Vault.
- Git host availability and successful push do not prove recoverability; only a
  fresh-clone restore drill does.

## Acceptance contract

The feature is accepted only when all of the following are freshly proven:

1. Unit and CLI tests show RED then GREEN for deterministic manifests,
   allowlisted inventory, corruption, extra files, links, atomic staging, and
   non-empty-target refusal.
2. Existing memory isolation, six-tool MCP, Chinese retrieval, publication,
   lifecycle, security, and release gates remain green.
3. A formal snapshot of the current 28 Editions is committed and pushed to a
   new private repository.
4. A clean clone plus encrypted capsule restores into a new temporary ART home.
5. The restored Vault reports exactly 28 current Editions, aligned search
   index, verified hashes, and successful representative Chinese and English
   recalls.
6. The restored Vault is accessible to fresh Codex and DSH test identities
   without exposing either Agent's private memory.
7. After that proof, the old `fantasyce/agent-knowledge-base` repository is
   deleted and verified absent. The new backup repository remains private and
   recoverable.
8. ART 0.1.1 is released from protected `main`, its assets and MCP Registry
   record are verified, and the installed Codex/DSH plugin uses the published
   binary.
9. Task-owned worktrees, plaintext recovery material, clones, test homes,
   sessions, processes, and temporary archives are removed; retained formal
   data and Git state are reported explicitly.

