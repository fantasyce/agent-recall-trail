# Architecture decisions

The release decisions are recorded individually:

- [ADR-0001](0001-per-agent-private-vault.md)
- [ADR-0002](0002-memory-and-knowledge-are-separate.md)
- [ADR-0003](0003-immutable-knowledge-edition.md)
- [ADR-0004](0004-admission-before-ranking.md)
- [ADR-0005](0005-thin-across-context.md)
- [ADR-0006](0006-clean-room-zero-copy.md)

The compact decision register below is retained as a quick index.

## ADR-001: one private database per Agent

Status: accepted. Visibility flags inside one shared private database were rejected. Physical files plus startup binding make the most important isolation property inspectable and testable.

## ADR-002: memory and knowledge are different object families

Status: accepted. Private memory optimizes for personal continuity and correction. Shared knowledge optimizes for review, redaction, stability, and distribution. Promotion is proposal plus human review, never a status flip on memory.

## ADR-003: immutable Knowledge Editions

Status: accepted. Publication creates new Markdown and manifest files. Revocation and replacement are new events. Historical evidence is not rewritten.

## ADR-004: deterministic local retrieval first

Status: accepted. Exact matching, tokens, Jieba, and CJK bigrams provide explainable Chinese/English recall without a service dependency. Embeddings remain an explicit future capability.

## ADR-005: stdio MCP with process-bound identity

Status: accepted. The RC has no network listener. One host child maps to one Agent and exposes six Agent-safe tools. Human operations stay in the CLI.

## ADR-006: thin optional cross-Agent contracts

Status: accepted. Recall Bundles and grants can be carried by a future coordinator, but ART remains the authority for private memory and shared Editions. The contracts cannot contain private bodies and expire by default.

## ADR-007: independent design and terminology

Status: accepted. Product invariants, schemas, names, tests, prompts, storage layout, and workflows are derived within ART. Public market research may identify user needs but cannot supply implementation structure or proprietary artifacts.
