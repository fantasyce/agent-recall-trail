# ART Retrieval Quality Design

**Status:** Proposed for operator review

**Target release:** ART 0.2.0

## Purpose

ART 0.1.1 correctly separates private per-Agent memory from human-reviewed
shared Knowledge Editions, but its retrieval layer does not yet preserve the
quality of its own SQLite FTS5 candidate ranking. A public BEIR probe on the
installed release showed that the overlap-only reranker and fixed three-result
cap materially reduce recall and nDCG relative to the underlying BM25 order.

This change makes ART a stronger lexical knowledge retriever without changing
memory ownership, knowledge governance, local-first operation, or the six-tool
MCP boundary. It establishes a stable ranked-candidate and rank-fusion seam for
a separately reviewed offline semantic retriever.

## Evidence and success criteria

The 2026-08-30 probe used complete BEIR SciFact and NFCorpus test splits through
the installed `art 0.1.1` CLI. The same documents and qrels produced:

| Dataset | System | Recall@3 | nDCG@3 | Recall@10 | nDCG@10 |
|---|---:|---:|---:|---:|---:|
| SciFact | ART 0.1.1 | 0.4753 | 0.4375 | 0.4753 | 0.4370 |
| SciFact | FTS5 BM25 control | 0.6723 | 0.6214 | 0.7989 | 0.6692 |
| NFCorpus | ART 0.1.1 | 0.0673 | 0.2226 | 0.0673 | 0.1350 |
| NFCorpus | FTS5 BM25 control | 0.1018 | 0.3607 | 0.1494 | 0.3093 |

ART 0.2.0 is accepted only if the final production path meets all of these on
the unchanged public datasets and qrels:

- SciFact Recall@10 is at least `0.76` and nDCG@10 is at least `0.64`.
- NFCorpus Recall@10 is at least `0.14` and nDCG@10 is at least `0.29`.
- Recall@3, MRR@3, and nDCG@3 do not regress from the 0.1.1 measurements above.
- All 64 existing Chinese golden queries still return the expected record in
  the first three results.
- Admission, expiry, dispute, revocation, Agent isolation, and no-persist
  behavior remain unchanged.
- A 5,000-Edition product-path run stays below 350 ms p95 on the target Mac,
  including CLI startup and Vault open.

These are release gates, not tuning targets. Parameters must not be adjusted to
memorize individual BEIR query IDs, document IDs, or qrels.

## Considered approaches

### Selected: BM25-first retrieval with bounded lexical fusion

SQLite FTS5 already computes term frequency, inverse document frequency, and
document-length normalization. ART will preserve that ranked order as its
dominant signal, then apply only bounded exact-phrase, query-coverage, memory
authority, and recency signals. The final score is derived from ranked-list
positions rather than raw BM25 values, so private and knowledge stores can be
fused without pretending their raw score scales are comparable.

This is the smallest change that directly addresses the measured defect and
works for English, Chinese, mixed identifiers, paths, and version strings
without a network or model asset.

### Rejected: tune the current overlap score

The existing score ignores inverse document frequency and term frequency. New
weights could improve one benchmark while continuing to promote common words
over rare discriminative terms. The BM25 control proves that a stronger signal
already exists in the product and should not be discarded.

### Deferred: bundle a local embedding model in this release

Dense retrieval is necessary for synonym and no-lexical-overlap queries, but it
is an independent subsystem. It requires an approved model and license, model
integrity and cache policy, vector schema and rebuild semantics, incremental
indexing, backup/restore treatment, offline failure behavior, and macOS/Linux
packaging acceptance. ART 0.2.0 will provide a ranked-source fusion seam, but
will continue to report `vector_status: unavailable` until a later semantic
retrieval design passes those gates. It must never download a model implicitly
during recall.

## Ranked candidate contract

`art-agent-store` and `art-knowledge` will return ranked candidates rather than
bare records:

```rust
pub struct RankedMemoryCandidate {
    pub artifact: MemoryArtifact,
    pub lexical_rank: usize,
}

pub struct RankedEditionCandidate {
    pub edition: EditionRecord,
    pub lexical_rank: usize,
}
```

Raw SQLite BM25 values stay inside the storage crate. `lexical_rank` starts at
one and is stable only within one query. This prevents callers from comparing
database-specific negative BM25 values across physically separate Vaults.

Candidate search always executes the broad escaped OR expression for all
normalized query terms. It must not stop after an exact whole-query phrase
returns any result, because doing so can suppress other relevant documents.
FTS5 orders candidates by `rank`, with deterministic ID ordering as the final
tie-breaker. Knowledge search continues to filter to current, non-revoked
Editions in SQL. Private results still pass through all eligibility checks
before final fusion.

The default candidate depth remains 512. The retrieval engine may request up
to 2,048 candidates when the caller asks for more than three final results.
This limit is internal and cannot be used to cross Agent or governance
boundaries.

## Ranking and fusion

`art-retrieval` will split query parsing, ranked-source fusion, and bundle
rendering into focused modules. Each accepted candidate receives:

- a dominant reciprocal-rank contribution `1 / (60 + lexical_rank)`;
- a bounded exact normalized phrase bonus;
- a smaller bounded distinct-token coverage bonus;
- the existing `0.75` authority multiplier for candidate memory;
- no freshness bonus for immutable Knowledge Editions; and
- a deterministic subject reference tie-breaker.

Exact and coverage bonuses together may not move a candidate more than five
lexical ranks ahead of its BM25 position. This invariant prevents the previous
overlap-only reranker from recreating the measured regression while preserving
the useful behavior of exact identifiers and Chinese bigrams.

The fusion API accepts multiple ranked sources even though 0.2.0 supplies one
lexical source. A future semantic source can be added with Reciprocal Rank
Fusion only after its own design and release gates are approved. Missing or
failed optional sources must be represented in diagnostics and never silently
masquerade as a successful hybrid query.

## Result depth and compatibility

`RecallRequest` adds optional bounded fields:

```rust
pub max_private_results: Option<usize>,
pub max_knowledge_results: Option<usize>,
```

Each value must be in `1..=20`. Omitted values preserve the ART 0.1.1 default
of at most three private and three knowledge results. The effective cap is the
minimum of the requested value and the existing token-budget allocation, so a
caller cannot bypass context-size protection.

The CLI adds `--max-private-results` and `--max-knowledge-results`. The MCP
`art_recall` input adds the same optional fields. Existing callers, tool names,
JSON-RPC behavior, and the `art.recall.v1` output schema remain compatible.
The BEIR gate uses `--budget-tokens 6000 --max-knowledge-results 10` so standard
top-ten metrics measure the real public product path rather than a test-only
function.

`RecallItem.score` remains a query-local ordering value, not a calibrated
probability. `match_reasons` adds `bm25_rank`, and retains exact, Jieba-token,
and CJK-bigram reasons when those signals are present.

## Data and migration behavior

No canonical memory, Knowledge Edition, proposal, review, event, manifest, or
backup schema changes. Existing FTS tables remain disposable projections and
are rebuilt from the same authoritative objects.

This release does not split title and body into new FTS columns. Column-weight
tuning is deferred until the BM25-first result is measured, because changing
the projection schema and the ranking policy in one experiment would obscure
which change caused an improvement or regression.

## Failure and security behavior

- Empty queries, invalid result depths, and invalid token budgets fail with
  stable `ART_INVALID_INPUT` errors before database access.
- FTS expressions remain quoted and generated only from normalized tokens;
  caller text is never concatenated into SQL.
- A corrupted or misaligned search projection remains visible through Doctor
  and fails closed according to existing index behavior.
- Disputed memory may produce a caution but never enter normal results.
- Expired, superseded, archived, revoked, or cross-Agent data cannot gain
  authority from a higher lexical rank.
- Query text, result bodies, Recall Bundles, and benchmark corpora are not
  persisted into formal memory or shared knowledge.

## Testing and benchmark contract

Implementation follows explicit RED to GREEN cycles:

1. Storage contract tests first prove that broad BM25 order is returned and an
   exact-phrase hit no longer suppresses other candidates.
2. Retrieval contract tests prove BM25 dominance, bounded exact boosts,
   configurable depth, token-budget caps, deterministic ties, and unchanged
   admission behavior.
3. CLI and MCP tests prove optional fields, stable defaults, validation, and
   output compatibility for both Codex and DSH identities.
4. A checked-in ignored BEIR harness consumes operator-downloaded SciFact and
   NFCorpus paths, verifies their published MD5 values, builds only temporary
   ART homes, invokes the compiled product path, and emits JSON metrics.
5. The complete workspace test suite, ignored 10k/5k performance contract,
   release security checks, and installed Codex/DSH E2E run only after the
   benchmark gates pass in the isolated worktree.

Public corpora, generated Vaults, model files, query outputs, and benchmark
results are never committed to the open-source repository. Only the harness,
dataset checksums, metric definitions, and non-sensitive aggregate acceptance
report may be committed.

## Delivery sequence

1. Preserve ranked BM25 candidates in both storage crates.
2. Replace overlap-only sorting with bounded rank fusion.
3. Add compatible result-depth controls to library, CLI, and MCP.
4. Check in and run the reproducible BEIR release gate.
5. Run full source, performance, packaging, installed-plugin, Codex, and DSH
   acceptance before changing the formal installation.
6. Design offline semantic indexing as a separate ART 0.3.0 proposal using the
   ranked-source seam and new semantic/no-overlap benchmark cases.
