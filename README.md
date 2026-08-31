# ART — Agent Recall Trail

ART is a local-first memory and reviewed-knowledge product for coding agents. Every Agent gets a physically separate private Recall Trail. Stable conclusions become human-reviewed, immutable Knowledge Editions that other Agents can retrieve without seeing private source identities or source bodies.

ART `0.3.0` supports Codex and DeepSeek Harness (DSH) over stdio MCP. It is standalone: no AAA adapter, daemon, cloud sync, bundled model, or autonomous publication is required.

## Product boundary

- Private memory belongs to one process-bound Agent identity.
- Shared knowledge contains only committed, reviewed Editions.
- Agents may capture, recall, read, provide feedback, and create proposals.
- Only a local human may approve, publish, revoke, supersede, archive, or make assurance decisions.
- Stored content is evidence, never executable instruction or authorization.

## Progressive recall

ART exposes one recall API with four explicit retrieval modes: `lexical`, `full_scan`, `semantic`, and `hybrid`. The default is always `lexical`. A compact `route` request can identify relevant private and shared topics before a bounded `recall`; exact bodies are returned only by `read`.

- `lexical` uses local BM25 plus exact, token, Jieba, and CJK-bigram signals.
- `full_scan` ranks every governance-eligible canonical record. It needs no embedding service and is intended for completeness-sensitive, smaller stores.
- `semantic` uses only an explicitly configured embedding endpoint and disposable local vector projections.
- `hybrid` combines lexical and semantic ranks. If the optional provider or projection is unavailable, ART returns the unchanged lexical result and reports the fallback.

ART's optional embedding adapter is supplied and operated by the user. ART does not bundle, download, train, select, or make quality claims for a model merely because its endpoint is compatible.

## Quick start

```bash
cargo build --release --locked
./target/release/art --home /an/explicit/art-home init --confirm
./target/release/art --home /an/explicit/art-home agent create --id codex-primary --host codex
./target/release/art --home /an/explicit/art-home doctor --agent codex-primary --json
```

Use [Codex integration](integrations/codex/README.md) or [DSH integration](integrations/dsh/README.md) to start one bound stdio child. Run `art --help` for the operator CLI.

## Install

Download the native archive and `SHA256SUMS` from the
[latest release](https://github.com/fantasyce/agent-recall-trail/releases/latest),
verify the archive, then run the included installer with the extracted binary:

```bash
bash scripts/install.sh --binary /absolute/path/to/art --confirm
art --version
art doctor --agent codex-primary --json
```

The installer keeps executable bytes under `~/.across/bin/art`, creates a
discovery link at `~/.local/bin/art`, and initializes owner-only ART data. It
does not edit Codex or DSH configuration. The thin Codex plugin lives under
`plugin/agent-recall-trail`; its MCP child is permanently bound to
`codex-primary`. See [operations](docs/operations.md) before migrating or
publishing knowledge.

ART 0.3.0 adds progressive routing, governed full scan, and optional semantic
and hybrid adapters while preserving deterministic Knowledge Vault backup,
encrypted recovery of local review authority, verified empty-home restoration,
and reproducible lexical BEIR gates. See
[operations](docs/operations.md#backup-and-disaster-recovery) before creating
the dedicated private Git repository.

Configuration precedence is `--home`, then `--config <file>`, then the owner-only user config at `~/.across/config/art/config.json`, then the built-in `~/.across` root. The root config accepts only `schema` and `home`. Optional embedding configuration is isolated at `<ART_HOME>/config/art/embedding/default.json`; tokens, when needed, live in a separate owner-only file.

## Documentation

- [Architecture](docs/architecture.md)
- [Memory and knowledge model](docs/memory-and-knowledge.md)
- [Operations](docs/operations.md)
- [Security model](docs/security-model.md)
- [Testing and acceptance](docs/testing.md)
- [Architecture decisions](docs/decisions/README.md)
- [Independent design review](docs/research/design-independence.md)

All automated or manual tests must use an explicit task-owned ART home. ART never modifies Codex or DSH configuration automatically.

Apache-2.0 licensed. See [Security](SECURITY.md),
[Contributing](CONTRIBUTING.md), and [Support](SUPPORT.md).
