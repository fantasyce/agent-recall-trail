# Operations

## Initialization and identity

Always pass `--home` in automation. For interactive use, precedence is `--home` > explicit `--config` > owner user config > built-in root. Config contains no credential fields. `art init --confirm` creates the root and a private commitment key. Create canonical lowercase Agent IDs with `art agent create`. Each MCP child is then started with one fixed `--agent` value.

`art doctor --agent <id> --json` reports binary/schema versions, profile and Vault binding, owner-only modes, SQLite integrity/foreign keys/WAL, migration checksum, record and search-index counts, index alignment, shared manifest/event hashes, stale proposals, pending publication recovery, FD count, and vector availability. `--repair-preview` returns exact human recovery targets; `--apply` is deliberately rejected because Doctor never mutates implicitly. `art reindex --agent <id>` rebuilds the private lexical projection and checkpoints WAL; `--knowledge` rebuilds the shared projection and lexical index from immutable files/events.

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

## Shutdown and recovery

The stdio server exits on EOF, SIGINT, or SIGTERM, rejects new requests with `ART_SHUTTING_DOWN`, and waits at most three seconds. Hosts own restart and reconnect policy. SQLite uses WAL, foreign keys, a five-second busy timeout, migration serialization, and explicit checkpoints. Complete hash-valid publish files are safely committed on restart; partial files move to `.art/recovery/<intent-id>` and keep health degraded for human inspection. Recovery never exposes a partial Edition.

## Upgrade and rollback

Back up the explicit ART home while no ART process is writing. Unknown newer private-store schema versions fail closed. Roll back the binary only when its schema support matches the stored data. Build and test scripts never install ART globally; `scripts/install.sh` requires an explicit confirmation.
