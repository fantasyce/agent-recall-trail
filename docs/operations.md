# Operations

## Initialization and identity

Always pass `--home` in automation. For interactive use, precedence is `--home` > explicit `--config` > owner user config > built-in root. Config contains no credential fields. `art init --confirm` creates the root and a private commitment key. Create canonical lowercase Agent IDs with `art agent create`. Each MCP child is then started with one fixed `--agent` value.

`art doctor --agent <id> --json` reports binary/schema versions, profile and Vault binding, owner-only modes, SQLite integrity/foreign keys/WAL, migration checksum, record and search-index counts, index and navigation alignment, shared manifest/event hashes, stale proposals, pending publication recovery, FD count, and vector availability. `--repair-preview` returns exact human recovery targets; `--apply` is deliberately rejected because Doctor never mutates implicitly. `art reindex --agent <id>` rebuilds the private lexical projection and checkpoints WAL; `--knowledge` rebuilds the shared projection and lexical index from immutable files/events. Add `--navigation` to rebuild the lane-local route maps.

## Retrieval modes and optional embedding

The same recall command supports all four modes:

```bash
art recall --agent codex-primary --mode lexical --detail route --json "release recovery"
art recall --agent codex-primary --mode lexical --json "release recovery"
art recall --agent codex-primary --mode full-scan --json "release recovery"
art recall --agent codex-primary --mode semantic --json "release recovery"
art recall --agent codex-primary --mode hybrid --json "release recovery"
```

Lexical is the default and needs no additional configuration. Full scan reads all governance-eligible canonical records. Semantic and hybrid are explicit opt-in modes. If their optional runtime is unavailable, the response reports the requested and effective modes plus a safe fallback reason and returns lexical results.

To connect a user-operated OpenAI-compatible HTTPS endpoint, create `<ART_HOME>/config/art/embedding/default.json` as a regular mode-`0600` file:

```json
{
  "schema": "art.embedding.endpoint.v1",
  "protocol": "openai_compatible",
  "endpoint": "https://embedding.example.invalid",
  "model": "operator-selected-model",
  "revision": "operator-pinned-revision",
  "dimensions": 1024,
  "normalized": true,
  "timeout_ms": 5000,
  "token_file": "/absolute/owner-only/token-file"
}
```

`revision`, `token_file`, and `ca_file` are optional. Runtime file paths must be absolute regular mode-`0600` files. ART rejects URL credentials, query strings, fragments, redirects, non-HTTPS endpoints, invalid dimensions, and oversized inputs or responses. It reads a token only when sending a request and never returns it in diagnostics.

Build current private and shared semantic projections explicitly:

```bash
art --home /absolute/art-home reindex \
  --agent codex-primary --knowledge --vectors
```

Progress is emitted as bounded JSON counters on stderr. Interrupted staging is retained and resumes completed batches; the previous complete projection remains active until the replacement validates and is installed atomically. Document collection must observe the same canonical epoch before and after its snapshot; concurrent canonical change rejects that rebuild instead of stamping old documents with a new epoch. Vectors are disposable and excluded from backup, restore, knowledge export, and public source control.

Hybrid fusion is also explicit and user-configurable. To override the versioned
default, create `<ART_HOME>/config/art/retrieval/fusion.json` as one regular
mode-`0600` file:

```json
{
  "version": "art.rank-fusion.v1",
  "lexical_weight": 1.0,
  "semantic_weight": 0.7,
  "rrf_k": 60
}
```

Weights must be finite and non-negative, at least one must be non-zero, and
`rrf_k` must be between 1 and 1000. An invalid policy disables the optional
semantic path with a safe diagnostic; it never silently changes ranking.

## Human workflow

1. Agent captures sourced private memory.
2. Human assures, disputes, supersedes, or archives through `art memory` commands.
3. Agent creates a knowledge proposal from exact memory references.
4. Human inspects the proposal and sources, then approves, requests changes, or rejects.
5. Human publishes with `--confirm`.
6. Human verifies or revokes an Edition. Existing Edition files remain immutable.

## Import and export

`art import markdown --source <path> --dry-run` emits deterministic Knowledge Import Proposals with source path/hash, title, permalink, wiki links, eligibility, and warnings for missing titles, duplicate permalinks, dangling links, or secret-like content. A write requires both `--copy-to <new-path>` and `--confirm`; blocked findings prevent copying. The destination must not exist, must be outside the source, and symbolic/hard links are rejected. Only Markdown is copied with private permissions. The source is never edited.

After a human reviews one exact Markdown file, `art knowledge proposal
compose-file` creates a `FileSnapshot`-locked proposal. It requires the
expected SHA-256 and rejects symbolic links, hard links, non-Markdown files,
unsafe source identifiers, and digest mismatches. The separate human
`knowledge review approve` and `knowledge publish --confirm` steps remain
mandatory; the MCP surface never exposes them.

For a complete reviewed tree, `scripts/migrate_markdown_knowledge.py` stages
the deterministic scan, composes each file separately, invokes the same human
review and publication commands only with `--confirm-reviewed`, and writes an
owner-only resumable receipt. `scripts/verify_migration_receipt.py` reconciles
the source inventory, every source and Edition digest, the complete current
Edition set, and ART's own Edition verifier. After an intentional source
retirement, `--allow-source-absent` verifies the immutable Editions and receipt
without weakening the count, uniqueness, or hash checks.

Private export uses `art.memory.export.v1` JSONL. Full artifacts and anchors require both `--include-private` and `--confirm`; re-import requires the exact Agent identity and `--confirm`, is idempotent by ID+revision, and rejects hash conflicts. Knowledge export copies immutable Edition and lifecycle files to a new owner-only directory; it excludes the private control database.

## Backup and disaster recovery

ART backup is an explicit owner operation, never an Agent or MCP action. First
create or verify a deterministic canonical snapshot:

```bash
art --home /absolute/art-home backup create --output /new/snapshot
art backup verify --source /new/snapshot
art backup restore --source /snapshot --target-home /absent/art-home \
  --commitment-key /secure/commitment.key --confirm
```

`create` includes only immutable Edition Markdown/manifests and lifecycle
events; navigation, lexical, and semantic projections are excluded. `verify` rejects missing, extra, unsupported, corrupt, symlinked, and
hard-linked content. `restore` requires an absent target and a regular 32-byte
mode-`0600` commitment key; it builds and verifies a sibling staging home
before the atomic rename.

For full disaster recovery, prepare a dedicated private Git repository and an
age or SSH recipient file, then run:

```bash
bash scripts/backup_knowledge_to_git.sh \
  /absolute/art /absolute/art-home /absolute/private-git-worktree \
  /absolute/recipients.txt owner/repository --confirm
git -C /absolute/private-git-worktree push origin main
```

The script checkpoints a consistent Control Store copy, combines it with the
commitment key, encrypts both into the recovery capsule, binds the capsule to
the knowledge tree hash, and commits only the documented allowlist. It never
configures a remote and never pushes automatically. A successful commit or
push is not recovery proof.

Test recovery from a fresh clone into a target that does not exist:

```bash
bash scripts/restore_knowledge_from_git.sh \
  /absolute/art /absolute/fresh-clone /secure/age-identity.txt \
  /absolute/absent-art-home --confirm
```

The restore workflow validates both manifests, decrypts into an owner-only
temporary directory, checks the SQLite backup, restores canonical knowledge,
installs the recovered audit authority, rebuilds the portable projection, and
runs Doctor. Plaintext recovery material is removed on success and failure.
Agent Vaults are deliberately outside this backup and need an independent
owner-approved private-memory backup policy if desired.

## Shutdown and recovery

The stdio server exits on EOF, SIGINT, or SIGTERM, rejects new requests with `ART_SHUTTING_DOWN`, and waits at most three seconds. Hosts own restart and reconnect policy. SQLite uses WAL, foreign keys, a five-second busy timeout, migration serialization, and explicit checkpoints. Complete hash-valid publish files are safely committed on restart; partial files move to `.art/recovery/<intent-id>` and keep health degraded for human inspection. Recovery never exposes a partial Edition.

## Upgrade and rollback

Back up the explicit ART home while no ART process is writing. Unknown newer private-store schema versions fail closed. Roll back the binary only when its schema support matches the stored data. Build and test scripts never install ART globally; `scripts/install.sh` requires an explicit confirmation.
