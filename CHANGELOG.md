# Changelog

## 0.3.4 - 2026-09-07

- Expose `art_memory_capture.anchors[].kind` as the exact eight-value MCP enum
  already used by the domain model.
- Reject unknown anchor kinds at input decoding with canonical alternatives
  instead of a generic runtime validation error.
- Document the source-anchor vocabulary in the bundled Codex and DSH skills.

## 0.3.3 - 2026-09-07

- Added an ART-owned loopback governance page for Codex Desktop and DSH settings, review, publication, status, and audit.
- Added persistent, default-off delegation scoped to the canonical host binding plus Agent identity.
- Added one exact `approve_and_publish` operation for an unambiguous current user instruction, with no Agent-controlled authority fields.
- Added distinct `AgentDelegated` approval and publication receipts and direct Submitted-to-Materialized publication with no visible approved-only state.
- Preserved separate MCP form Elicitation operations for compatible clients while removing human-facing terminal fallback from Codex and DSH guidance.
- Expanded the bounded MCP surface to eight tools with `art_governance_ui_open`.

## 0.3.2 - 2026-09-06

- Added `art_knowledge_governance` as the seventh Agent-safe MCP tool.
- Added separate MCP form Elicitations for human proposal review and immutable Edition publication.
- Kept decisions, reasons, reviewer identity, and publication confirmation out of Agent-controlled tool arguments.
- Added exact Proposal snapshot revalidation, stable unsupported-client fallback, non-consent handling, replay protection, and unchanged independent-review requirements for elevated knowledge.
- Added protocol-level paired-client coverage and updated Codex/DSH guidance to prohibit treating ordinary chat as approval or executing CLI fallback decisions for the user.

## 0.3.1 - 2026-09-06

- Fixed Codex plugin startup when GUI or restricted SSH hosts omit the user-local executable directory from `PATH`.
- Added a package-relative POSIX launcher that preserves explicit `PATH` installations and falls back to the standard `~/.local/bin/art` link.
- Corrected the Codex skill's feedback tool name and documented recall parameter bounds and default behavior in both Codex and DSH guidance.
- Added launch, reconnect, missing-installation, argument-preservation, and exact MCP surface regression coverage.
- Clarified navigation alignment diagnostics and the host cache refresh required after plugin changes.
- Made the release gate honor an isolated Cargo target directory.

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
