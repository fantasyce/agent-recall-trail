# ART Remote Semantic Retrieval Design

> **Superseded on 2026-08-31. Do not implement this design.** It is retained as
> historical experiment context. The current ART 0.3 architecture contract is
> `2026-08-31-art-progressive-recall-architecture-design.md`.

**Status:** Superseded

**Target release:** ART 0.3.0

## Purpose

ART 0.2.0 has a strong local BM25 path, but lexical retrieval cannot recover a
relevant memory or Knowledge Edition when a query uses different vocabulary.
The operator has approved connecting the formal local ART runtime to the
Qwen3-Embedding-0.6B service already accepted on the old Intel MacBook over the
private Thunderbolt link, then repeating the public BEIR experiment.

Embedding is an optional retrieval signal, not a replacement for ART's source
of truth or governance. Private Agent memory and reviewed shared knowledge stay
in their existing authoritative stores. Vector files are disposable local
projections and never enter knowledge backup Git.

## Success criteria

- The same SciFact and NFCorpus documents, test queries, qrels, metric code, and
  top-k settings compare the exact ART 0.2.0 lexical path with the candidate
  hybrid path.
- Hybrid Recall@10 and nDCG@10 must not regress on either dataset and should
  improve the macro average. The report records absolute and relative deltas;
  no claim of improvement is allowed without measured evidence.
- All existing Chinese golden, admission, Agent-isolation, revocation, MCP,
  CLI, backup, install, Codex, and DSH tests remain green.
- A healthy configured service reports `vector_status: ready`. Missing,
  stale, version-mismatched, timed-out, busy, or unreachable semantic
  retrieval returns the unchanged lexical result with a non-ready status and a
  safe caution; recall itself remains available.
- Hybrid query p95, including CLI startup and the Thunderbolt service call,
  remains below 800 ms on both public datasets.
- No token, private query, memory body, corpus, raw ranking, vector, or formal
  ART database is committed to the public repository.

## Considered approaches

### Selected: local isolated vector projections plus weighted RRF

The MacBook service generates normalized Qwen vectors. ART stores them locally
in two physically separate SQLite projections: one beside each Agent Vault and
one below the private `.art/retrieval` area of the Knowledge Vault. Recall
embeds only the query remotely, obtains independent lexical and dense ranked
lists, and combines them with deterministic weighted reciprocal-rank fusion.
Lexical rank has weight `1.0`; dense rank has weight `0.7`; `k=60`. Existing
bounded exact/token/bigram bonuses and memory authority remain tie-breaking
signals. These parameters are global and may not depend on dataset, query ID,
document ID, or qrels.

This keeps exact identifiers, paths, and Chinese terms strong while adding
semantic/no-overlap candidates. It also preserves ART operation when the model
host is unavailable.

### Rejected: dense-only replacement

Dense-only retrieval can lose exact identifiers and makes the remote service a
hard runtime dependency. It is useful as an experiment control, not the formal
product behavior.

### Rejected: store vectors on the MacBook

That would centralize private Agent material outside its Vault boundary and
make the model host a second data authority. The remote service must remain
stateless and must not log request bodies.

## Configuration and trust

`art.config.v1` gains optional `embedding_endpoint`, an absolute path to an
`art.embedding.endpoint.v1` JSON file. When callers use `--home`, ART also
discovers `<ART_HOME>/config/art/embedding/default.json`. The endpoint file
contains only the HTTPS URL, expected model/revision/dimensions, timeout, CA
path, and bearer-token file path. The bearer value is read from an owner-only
regular file, never accepted inline, serialized, logged, or returned.

The client uses the configured private CA, rejects redirects, caps request and
response sizes, validates the response schema, count, dimensions, finite
values, model, and exact revision, and applies a 650 ms total timeout. It sends
at most 32 documents per indexing batch. Recall never downloads a model.

Private query and document text cross only the operator-controlled Thunderbolt
link to the stateless local service. The service is already TLS- and
Bearer-protected, bound to `10.77.77.2`, and suppresses request logging. This is
an explicit change from ART 0.2.0's no-network runtime and is documented as a
same-operator local processing boundary, not cloud replication.

## Projection contract

Each row stores `subject_ref`, content SHA-256, normalized little-endian f32
vector bytes, model, revision, dimensions, source epoch, and creation time.
Private projection path:

```text
data/art/agents/<agent-id>/art-vectors.sqlite3
```

Shared projection path:

```text
data/art/knowledge-vault/.art/retrieval/art-vectors.sqlite3
```

Files and parent directories are owner-only. Projection metadata binds the
exact provider fingerprint and the Vault's existing `index_epoch`. ART treats
any mismatch as `stale` and uses lexical retrieval until the operator runs:

```text
art reindex --agent <id> --knowledge --vectors
```

Rebuild writes a task-owned staging database, validates row counts and vector
shape, then atomically replaces the old projection. Failure preserves the
last complete projection but marks it unusable when its epoch no longer
matches. Backups and restores exclude vectors; reindex reconstructs them.

## Recall data flow

1. Validate the query and result depths before storage or network access.
2. Apply existing lexical candidate retrieval independently in the private and
   shared lanes.
3. If configured projections are current, request one query vector and rank
   each lane by cosine similarity using its own projection.
4. Union lexical and dense candidates inside the same lane, then load canonical
   objects and apply eligibility, expiry, dispute, current/revoked, and
   cross-Agent checks before fusion.
5. Fuse ranks, preserve distinct private/shared output arrays, enforce token
   budgets, and emit a non-secret vector status.
6. On any optional semantic error, return the lexical bundle and record a safe
   status/caution without service error bodies.

Dense rank can introduce a no-overlap record, so lexical match is no longer an
admission requirement when a valid dense rank exists. Governance admission is
always required before either rank contributes.

## Diagnostics and compatibility

`art recall`, `art_recall`, and `art.recall.v1` remain compatible.
`vector_status` becomes one of `unavailable`, `ready`, `stale`, or `degraded`.
Doctor reports configuration presence, provider fingerprint, per-lane row
counts, epoch alignment, and reachability without returning paths containing
credentials or making a query request. The six-tool MCP boundary is unchanged.

## Test and experiment contract

Implementation follows explicit RED to GREEN cycles for endpoint parsing,
credential permissions, response validation, projection isolation and atomic
rebuild, epoch staleness, rank fusion, semantic no-overlap recall, lexical
fallback, CLI/MCP wiring, and Doctor output.

The BEIR harness gains a paired mode. It builds one disposable fixture per
dataset, records the 0.2.0-compatible lexical result, rebuilds a candidate
vector projection through the formal endpoint, records hybrid results, and
emits aggregate metrics plus deltas and latency. Dataset and query order are
fixed. The public report contains aggregates only. A dense-only control may be
computed by the harness to explain results but is never the product default.

Formal cutover occurs only after paired BEIR gates, full workspace and release
gates, installed-binary verification, real Codex and DSH E2E, service restart
fallback, and residue review pass. If quality regresses, ART 0.2.0 remains the
formal installation and the candidate is not released.
