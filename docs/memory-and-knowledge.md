# Memory and knowledge model

## Private memory

ART has four payloads:

- Episode: situation, action, outcome, and learning.
- Semantic: claim, applicability, and exceptions.
- Procedure: prerequisites, steps, verification, and rollback.
- Decision: context, choice, alternatives, consequences, and revisit condition.

Every memory also has a title, summary, scope, sensitivity, status, immutable revision history, validity fields, content hash, and one or more source anchors unless it remains an explicit unanchored Candidate.

Eligible default recall includes Active memory only. Candidate requires an explicit option. Disputed, Superseded, Rejected, Archived, expired, and invalidated records are filtered before ranking.

## Source anchors and assurance

Anchors are bounded references across eight declared kinds. Each carries a safe locator, optional source version/digest, bounded excerpt hash, sensitivity, observation time, and canonical content hash. Secret-like content, raw transcripts, unverified success booleans, and oversized excerpts are rejected. Assurance decisions bind `(memory_id, revision, anchor_set_hash, actor, rationale)` and are append-only. A source digest change or revocation appends a source event, marks the current memory non-eligible, and invalidates its use as a proposal source.

## Shared knowledge

A proposal contains a draft plus exact source locks. Proposal and review data never enter shared retrieval. Publication produces:

- immutable Markdown containing applicability and reviewed knowledge;
- a redacted manifest containing hashes, commitments, review receipt hash, schema version, and generator version;
- a local projection marking the newest Edition current.

External Markdown follows the same lifecycle. A human CLI operator may create
a proposal whose source lock is `FileSnapshot`, has no owning Agent, and binds
an exact relative identifier plus content SHA-256. The Markdown becomes shared
only after separate human approval and publication. It is never inserted into
an Agent Vault or treated as a promoted private memory.

The manifest omits Agent IDs, private memory IDs, source locators, and excerpts. Revocation is a new immutable event and never rewrites an Edition. Startup reconciliation completes events that reached disk before their projection transaction and fails closed if an already-applied event disappears or changes.

## Recall Bundle

A Recall Bundle has separate `private_memories` and `knowledge_editions` arrays, a query hash, generation and expiry times, omissions, cautions, token budget, and `persist_policy=no_automatic_capture`. The Bundle is temporary output and must not become a new memory automatically.

## Ranking

ART v0.2.0 uses deterministic local retrieval: FTS5 BM25 ordering fused with bounded normalized exact, Unicode-aware token, Jieba segmentation, and CJK-bigram signals. Private and shared stores maintain separate persistent projections, select bounded candidates locally, and evaluate eligibility before ranking. Operators may request 1 through 20 private and shared results; the default remains three per lane and the token budget still bounds output. Both projections are rebuildable from authoritative memory artifacts or immutable Knowledge Editions and events. Vector status is explicitly `unavailable`; there is no silent embedding fallback.
