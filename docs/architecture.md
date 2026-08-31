# Architecture

ART separates private experience from shared knowledge instead of treating them as one database with visibility flags.

## Runtime components

1. `art-domain` defines Agent identity, four typed memory payloads, source anchors, assurance, proposals, Editions, grants, and Recall Bundles.
2. `art-agent-store` owns one SQLite file per Agent. The database stores revisions, versioned/digested anchors, assurance decisions, relations, idempotent feedback, source changes, and lifecycle events.
3. `art-knowledge` owns a private control store and a shareable Edition tree. Proposals and source locks remain private; Edition Markdown and manifests contain commitments only.
4. `art-retrieval` implements one progressive recall pipeline over private and shared lanes. It uses rebuildable navigation, lexical, and optional semantic projections while canonical Agent artifacts and immutable shared files/events remain authoritative.
5. `art-mcp` binds one Agent identity at process start and exposes exactly six Agent-safe tools over stdio.
6. `art-cli` exposes human operations, diagnostics, integration previews, import/export, and reindex entry points.

## Trust flow

Capture begins as Candidate. A deterministic sourced capture may become Active, but Active means eligible for recall—not proven truth. An exact `memory_id` plus `expected_revision` creates a new immutable revision transactionally. Assurance decisions bind an exact memory revision and anchor-set hash. Source digest/revocation events make affected memory disputed and stale for proposals. Dispute, supersede, and archive preserve the old artifact and append an event.

An Agent proposal locks exact source revisions and hashes. A local human reviews the proposal. Publication requires a matching current approval and writes a new immutable Edition plus a redacted manifest. A recoverable intent spans the SQLite/filesystem boundary: partial files are quarantined, complete hash-valid files are projected, and only committed projections are recallable. Immutable revocation/replacement events are reconciled on startup.

## Physical layout

```text
<ART_HOME>/
  config/art/agents/<agent-id>.json
  config/art/embedding/default.json               # optional, owner-created
  config/art/commitment.key
  data/art/agents/<agent-id>/art.sqlite3
  data/art/agents/<agent-id>/retrieval/semantic.sqlite3   # optional projection
  data/art/knowledge-vault/art-control.sqlite3
  data/art/knowledge-vault/editions/<knowledge-key>/<number>-<edition-id>.{md,json}
  data/art/knowledge-vault/.art/events/*.json
  data/art/knowledge-vault/.art/retrieval/semantic.sqlite3 # optional projection
  data/art/knowledge-vault/.art/recovery/<intent-id>/
```

The MCP request never supplies owner identity. Starting a different Agent against an existing private database fails closed.

## Retrieval layers

ART keeps five logical layers with distinct authority:

1. canonical private memory and reviewed shared knowledge;
2. bounded lane-local navigation metadata used by `route`;
3. local lexical indexes used by the default `lexical` mode;
4. optional disposable semantic projections used only for `semantic` and `hybrid`;
5. one recall orchestrator that applies identity, lifecycle, review, validity, result, and token-budget policy before returning a Bundle.

The intended flow is `route -> recall -> read`. Route output contains topics, counts, and safe references—not memory or knowledge bodies. Recall returns bounded excerpts with diagnostics. Read resolves one exact admitted reference from the canonical store.

The user selects `lexical`, `full_scan`, `semantic`, or `hybrid` per request. Full scan traverses every governance-eligible canonical record after lane admission. Semantic modes never become active merely because a configuration file exists; they require an explicit request and current provider-bound projections. Any provider, network, dimension, or projection failure falls back to the same lexical result path with a safe status and reason.

## Deferred by design

ART v0.3.0 has no network listener, background daemon, bundled embedding model, cloud replication, hostile same-user sandbox, automatic physical deletion, or automatic knowledge approval. An optional embedding client makes outbound HTTPS requests only when the operator creates a valid endpoint configuration and explicitly selects a semantic mode or rebuilds vectors. There is no AAA runtime adapter in this release. Shared Markdown, Edition manifests, lifecycle events, and their private Git history are authoritative; navigation, search, current-state, and semantic projections are portable and rebuildable.
