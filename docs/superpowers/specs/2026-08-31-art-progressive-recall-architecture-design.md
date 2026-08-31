# ART Progressive Recall and Memory-Knowledge Architecture

**Status:** Draft for operator review

**Target release:** ART 0.3.0

**Supersedes:** `2026-08-31-art-remote-semantic-retrieval-design.md`

## Purpose

ART 0.2.0 already presents private Agent memory and reviewed shared knowledge
through one Recall Bundle, but its read path is still a single lexical pass. ART
0.3.0 will make that unified product surface progressively layered while
preserving two different authority domains:

- private memory remains owned by exactly one process-bound Agent;
- shared knowledge remains a human-reviewed, immutable Knowledge Edition;
- both are recalled through one bounded API without becoming one object family,
  one database, or one visibility flag.

Embedding is retained as an optional retrieval adapter. ART does not bundle a
model, require a model for normal operation, or make provider recall quality a
claim about ART itself.

## Architectural invariant

ART uses one product, one recall entry point, and one traceable evolution path,
but keeps two stores and two governance models.

```text
source evidence
    -> private Agent memory
        -> exact-source Knowledge Proposal
            -> human review
                -> immutable shared Knowledge Edition

private memory -----------\
                           -> unified Recall Bundle -> exact read on demand
shared Knowledge Edition -/
```

"Unified" means common discovery, explanation, budgeting, and delivery. It
never means automatic promotion, shared private storage, or a mutable object
changing from private memory into public knowledge.

## Considered approaches

### Selected: dual-authority progressive recall

Keep the existing private and shared object families, add separate rebuildable
navigation projections for each lane, and route both through one progressive
Recall Engine. This preserves ART's isolation and review guarantees while
making memory and knowledge feel like one coherent product.

### Rejected: one memory-to-knowledge object lifecycle

Changing a private record's status until it becomes shared would conflate
ownership, assurance, publication, revocation, and visibility. It would also
make cross-Agent access depend on mutable flags rather than physical and
cryptographic boundaries.

### Rejected: retrieval adapters without a navigation layer

Only adding full-scan or embedding switches would multiply ranking paths but
would not improve context budgeting, topic discovery, provenance, or the
memory-to-knowledge experience. Retrieval adapters belong below a stable
progressive read path.

## Logical layers

### 1. Evidence layer

Source Anchors remain bounded references with versions, digests, observation
times, sensitivity, and safe locator metadata. ART never stores full
transcripts, unrestricted command output, secrets, or recalled bundles as
evidence.

### 2. Private memory layer

Episode, Semantic, Procedure, and Decision memories remain immutable revisions
inside one physical Agent Vault. Capture remains explicit and process-bound.
Active means eligible for recall, not verified truth. Candidate, disputed,
expired, superseded, rejected, and archived states retain their existing
admission rules.

### 3. Navigation layer

ART adds two physically separate, disposable projections:

- an Agent Recall Map derived only from that Agent's eligible private memories;
- a Shared Knowledge Catalog derived only from current, non-revoked Knowledge
  Editions.

The projections contain only fields deterministically derived from canonical
metadata. Private entries use memory kind, scope type/key, title terms, status,
revision, recency, and usage counters. Shared entries use knowledge key, title,
applicability, Edition number/status, and usage counters. Both carry subject
references and source epochs. They do not contain raw source bodies, private
cross-Agent identifiers, proposal data, review data, or new authority.

Maps are deterministic and rebuildable from canonical artifacts. Updating a
map never rewrites a memory or Edition. A missing, corrupt, or stale map causes
ART to rebuild it or continue through canonical lexical/full-scan retrieval;
it never makes recall unavailable.

### 4. Knowledge governance layer

An Agent may draft a Knowledge Proposal from exact source revisions visible to
that Agent. External files continue to use exact FileSnapshot locks. Only a
local human operator may approve, reject, request changes, publish, revoke, or
supersede knowledge. Publication creates a new immutable Edition and updates
the shared catalog projection.

### 5. Unified recall layer

`art_recall` remains the single discovery entry point and preserves separate
`private_memories` and `knowledge_editions` arrays. `art_read` remains the only
way to expand an exact result into its permitted full representation.

The read path has three progressive levels:

1. **Route:** consult bounded lane-local Recall Map/Catalog metadata to identify
   likely topics and applicable scopes.
2. **Recall:** run the selected retrieval policy inside each lane, apply
   governance admission, rank candidates, and return bounded excerpts.
3. **Read:** load an exact private revision or visible Knowledge Edition by
   `subject_ref` only when the Agent needs the detail.

`RecallRequest` gains an additive `detail` value:

- `route` returns bounded navigation topics and counts without result bodies;
- `recall` runs candidate retrieval and returns bounded excerpts, preserving
  the ART 0.2 behavior and remaining the default.

Exact `art_read` is the third level and does not need another request mode.

No private and shared bodies are concatenated into an always-loaded global
summary. Codex and DSH integrations continue to instruct the Agent when to call
ART; the host does not receive another Agent's navigation projection.

## Retrieval policy

ART exposes four explicit modes through the library, CLI, MCP request schema,
and operator configuration:

| Mode | Candidate source | Embedding required | Availability behavior |
|---|---|---:|---|
| `lexical` | FTS5 BM25 plus exact, token, Jieba, and CJK-bigram signals | No | Default |
| `full_scan` | Every governance-eligible canonical record in the selected lane | No | Always available, slower |
| `semantic` | Optional semantic projection | Yes | Falls back to `lexical` with an explicit status |
| `hybrid` | Lane-local lexical and semantic ranks | Yes | Falls back to unchanged `lexical` results |

The configured default is `lexical`. A caller may select another mode per
request. ART never silently enables semantic retrieval merely because an
endpoint file exists.

Full scan means a complete scan only after Agent identity, lifecycle,
sensitivity, expiry, revocation, and current-Edition admission. It is not a
global database scan and cannot cross another Agent Vault.

Every Recall Bundle records:

- requested retrieval mode;
- effective retrieval mode;
- per-lane candidate sources;
- safe fallback reason, if any;
- projection freshness/status;
- bounded match reasons and source references.

## Optional embedding adapter

ART defines a provider-neutral `EmbeddingProvider` boundary rather than a
Qwen-specific product path. A provider supplies:

- a stable provider and model fingerprint;
- vector dimensions and normalization declaration;
- bounded document and query embedding operations;
- explicit timeout and failure results without secret-bearing bodies.

ART 0.3.0 ships one documented OpenAI-compatible HTTPS adapter behind this
interface. Its configuration declares the endpoint URL, model, optional
revision, dimensions, normalization, timeout, private CA file, and owner-only
bearer-token file. The core interface does not encode a model family,
quantization, host address, benchmark corpus, or fixed ranking weight.

When configured, ART stores vectors in disposable, owner-only projections:

```text
data/art/agents/<agent-id>/retrieval/semantic.sqlite3
data/art/knowledge-vault/.art/retrieval/semantic.sqlite3
```

Projection metadata binds the canonical source epoch and exact provider
fingerprint. Vectors are excluded from public source control, private knowledge
Git, backup archives, manifests, and migration exports. They are rebuilt from
canonical private memories or immutable Editions.

ART validates adapter protocol, isolation, dimensions, finite values,
freshness, fallback, and deterministic fusion behavior. It does not certify a
provider's semantic quality. Provider or operator evaluation may use the BEIR
harness, but a weak optional model does not make ART's lexical/full-scan product
path fail release acceptance.

## Consolidation and maintenance

ART does not copy Codex's background transcript summarization. Instead it
adopts the separation of write-time processing and read-time retrieval while
keeping explicit, bounded capture:

1. Capture validates one typed private memory and exact anchors.
2. Storage appends the canonical revision and lifecycle event.
3. Maintenance incrementally refreshes the Agent Recall Map and lexical
   projection.
4. Feedback appends evidence and may update derived usage counters, but never
   silently edits authority or content.
5. A Knowledge Proposal explicitly locks exact memory/file revisions.
6. Human publication refreshes the Shared Knowledge Catalog and lexical
   projection.

`art reindex` remains the recovery operation for every derived projection.
Maintenance may suggest duplicate, stale, or low-value records, but automatic
merging, deletion, knowledge promotion, approval, and publication remain out of
scope.

## Compatibility

- The six MCP tools remain unchanged: `art_recall`, `art_read`,
  `art_memory_capture`, `art_knowledge_propose`, `art_feedback`, and
  `art_health`.
- Existing callers that omit retrieval mode receive the ART 0.2-compatible
  lexical path.
- Existing `private_memories`, `knowledge_editions`, token-budget,
  `persist_policy`, and `vector_status` fields remain.
- New retrieval diagnostics are additive and non-secret.
- Codex and DSH remain equal first-class integrations; no AAA adapter is added.

## Failure behavior

- A failed navigation projection falls back to canonical retrieval and reports
  `map_status=stale|degraded`.
- A missing semantic configuration reports `vector_status=unavailable` and
  uses lexical retrieval.
- A stale semantic projection reports `vector_status=stale` and uses lexical
  retrieval until explicit reindex.
- Timeout, authentication, rate-limit, malformed response, wrong fingerprint,
  or dimension mismatch reports `vector_status=degraded` without returning
  service error bodies.
- Full scan evaluates every admitted canonical record before ranking and then
  bounds only the returned bundle. If an explicit operator safety cap prevents
  completion, ART reports `full_scan_incomplete`, uses the lexical path, and
  records `effective_mode=lexical`; it never labels a partial scan as complete.
- Corrupt canonical memory or knowledge continues to fail closed under the
  existing storage and Edition integrity rules.

## Security and privacy

- Private Recall Maps, lexical indexes, and semantic projections stay under the
  owning Agent directory with owner-only permissions.
- Shared projections contain only committed, current, non-revoked Editions.
- Query or document text sent to an optional embedding endpoint is an explicit
  operator-configured processing boundary. ART never enables it automatically.
- Endpoint credentials are file references to owner-only regular files; values
  are never serialized, logged, returned, or accepted inline.
- Candidate generation never occurs across private Agent lanes. Admission is
  required before any score can affect visible ordering.
- Retrieved content remains evidence, never executable instruction.

## Testing and acceptance

Implementation follows explicit RED-to-GREEN cycles and uses only task-owned
ART homes, datasets, endpoint stubs, projections, processes, and ports.

Required coverage:

1. deterministic Recall Map/Catalog rebuild, corruption, epoch, and isolation;
2. lexical compatibility with ART 0.2.0;
3. complete governance-filtered full scan in private and shared lanes;
4. explicit mode selection and safe fallback diagnostics;
5. provider-neutral embedding contract with deterministic stub vectors;
6. semantic and hybrid admission before ranking and no cross-Agent leakage;
7. unchanged proposal, human review, publication, revocation, backup, and
   recovery behavior;
8. stable six-tool MCP surface and CLI compatibility;
9. real installed Codex and DSH progressive recall/read journeys;
10. residue audit and cleanup of test homes, vectors, endpoint processes,
    datasets, build artifacts, and temporary configuration.

The public BEIR lexical gates remain release-blocking. Semantic quality reports
are provider-qualified evidence and never replace lexical/full-scan acceptance.

## Documentation migration

The previous Qwen-specific remote semantic design and plan are retained only as
historical experiment records and marked superseded. Implementation must update
architecture, memory/knowledge, retrieval testing, operations, security,
integration, changelog, and release documents together so no public surface
describes embedding as required or as ART's primary 0.3 direction.

## Non-goals

- bundling, downloading, training, or selecting an embedding model;
- claiming a universal recall percentage for third-party providers;
- automatic transcript ingestion;
- automatic memory-to-knowledge promotion;
- merging private and shared authority into one database;
- introducing a background network daemon or AAA-specific runtime;
- changing human-only knowledge approval or publication.
