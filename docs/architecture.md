# Architecture

ART separates private experience from shared knowledge instead of treating them as one database with visibility flags.

## Runtime components

1. `art-domain` defines Agent identity, four typed memory payloads, source anchors, assurance, proposals, Editions, grants, and Recall Bundles.
2. `art-agent-store` owns one SQLite file per Agent. The database stores revisions, versioned/digested anchors, assurance decisions, relations, idempotent feedback, source changes, and lifecycle events.
3. `art-knowledge` owns a private control store and a shareable Edition tree. Proposals and source locks remain private; Edition Markdown and manifests contain commitments only.
4. `art-retrieval` queries physically separate persistent FTS5 projections, then applies eligibility filters before exact, token, Jieba, and CJK-bigram ranking. The projections are never authoritative and rebuild independently from private artifacts or immutable shared files/events. Private and shared result lanes remain distinct.
5. `art-mcp` binds one Agent identity at process start and exposes exactly six Agent-safe tools over stdio.
6. `art-cli` exposes human operations, diagnostics, integration previews, import/export, and reindex entry points.

## Trust flow

Capture begins as Candidate. A deterministic sourced capture may become Active, but Active means eligible for recall—not proven truth. An exact `memory_id` plus `expected_revision` creates a new immutable revision transactionally. Assurance decisions bind an exact memory revision and anchor-set hash. Source digest/revocation events make affected memory disputed and stale for proposals. Dispute, supersede, and archive preserve the old artifact and append an event.

An Agent proposal locks exact source revisions and hashes. A local human reviews the proposal. Publication requires a matching current approval and writes a new immutable Edition plus a redacted manifest. A recoverable intent spans the SQLite/filesystem boundary: partial files are quarantined, complete hash-valid files are projected, and only committed projections are recallable. Immutable revocation/replacement events are reconciled on startup.

## Physical layout

```text
<ART_HOME>/
  config/art/agents/<agent-id>.json
  config/art/commitment.key
  data/art/agents/<agent-id>/art.sqlite3
  data/art/knowledge-vault/art-control.sqlite3
  data/art/knowledge-vault/editions/<knowledge-key>/<number>-<edition-id>.{md,json}
  data/art/knowledge-vault/.art/events/*.json
  data/art/knowledge-vault/.art/recovery/<intent-id>/
```

The MCP request never supplies owner identity. Starting a different Agent against an existing private database fails closed.

## Deferred by design

ART v0.1.1 has no network listener, background daemon, embeddings, cloud replication, hostile same-user sandbox, automatic physical deletion, or automatic knowledge approval. Recall uses local persistent lexical projections and reports cold and steady-state latency separately. Across Context schemas are thin optional contracts only; there is no AAA runtime adapter in this release. Shared Markdown, Edition manifests, lifecycle events, and their private Git history are authoritative; SQLite search/current projections are portable and rebuildable.
