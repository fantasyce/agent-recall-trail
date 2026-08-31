# Changelog

## 0.3.0 - 2026-08-31

- Added progressive `route -> recall -> read` retrieval with bounded, lane-local navigation projections.
- Added four explicit user-selected modes: lexical, governed full scan, optional semantic, and optional hybrid retrieval.
- Added provider-neutral OpenAI-compatible embedding configuration, isolated disposable vector projections, resumable rebuilds, and explicit health/fallback diagnostics.
- Preserved the unchanged lexical result path when semantic retrieval is unconfigured or unavailable.
- Preserved physical per-Agent memory isolation, immutable human-reviewed shared Knowledge Editions, the exactly six-tool MCP surface, and deterministic backup/recovery.

## 0.2.0 - 2026-08-31

- Added BM25-ranked broad candidate retrieval for private memories and shared Knowledge Editions.
- Added BM25-first fusion with bounded exact, Jieba-token, and CJK-bigram signals.
- Added optional private and knowledge result depths from 1 through 20 across the library, CLI, and MCP schema.
- Added reproducible full-split BEIR SciFact and NFCorpus product-path quality gates.
- Preserved lexical-only operation, per-Agent isolation, six MCP tools, backup compatibility, and explicit `vector_status=unavailable`.

## 0.1.1 - 2026-08-30

- Added deterministic, strictly allowlisted Knowledge Vault snapshots.
- Added atomic empty-home restore with portable, rebuildable projections.
- Added an age-encrypted Control Store and commitment-key recovery capsule.
- Added explicit private-Git backup and fresh-clone disaster-recovery workflows.
- Preserved per-Agent memory isolation and the exactly six-tool MCP surface.

## 0.1.0 - 2026-08-30

- Added physically separate private Agent Vaults for Codex and DSH identities.
- Added source-anchored Episode, Semantic, Procedure, and Decision memory.
- Added immutable, human-reviewed Knowledge Editions shared across ART Agents.
- Added explainable Chinese and English lexical recall without an embedding service.
- Added six Agent-safe stdio MCP tools, operator CLI, Codex plugin, and DSH overlay.
- Added reviewed Markdown migration with resumable, hash-reconciled receipts.
- Added lifecycle, recovery, stress, performance, security, and real-host E2E gates.
