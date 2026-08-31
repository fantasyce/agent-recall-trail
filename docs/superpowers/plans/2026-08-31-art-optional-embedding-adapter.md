# ART Optional Embedding Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve user-selectable semantic and hybrid recall through a provider-neutral, fail-open embedding adapter without making any model or benchmark result part of ART's core correctness claim.

**Architecture:** `art-retrieval` defines a synchronous bounded `EmbeddingProvider` contract, one OpenAI-compatible HTTPS adapter, and disposable lane-local semantic projections. The Recall Engine performs governance admission before semantic scores become visible and falls back to the unchanged lexical path for every optional-provider failure.

**Tech Stack:** Rust 1.98, reqwest blocking/rustls/json, serde, rusqlite, SHA-256, tempfile

**Spec:** `docs/superpowers/specs/2026-08-31-art-progressive-recall-architecture-design.md`

## Global Constraints

- No bundled, downloaded, selected, or trained model.
- No fixed Qwen model, quantization, host, dimension, or fusion weight in core code.
- Semantic configuration is explicit; endpoint presence never changes the default lexical mode.
- Private and shared vectors remain physically separate, owner-only, disposable, and excluded from backup/Git/export.
- Missing, stale, busy, timed-out, unauthorized, malformed, or mismatched providers return lexical results with safe diagnostics.
- Provider quality is operator/provider evidence, not an ART release claim.
- Tests use deterministic local stubs, task-owned ports, and task-owned ART homes.

---

### Task 1: Define provider-neutral configuration and interface

**Files:**
- Create: `crates/art-retrieval/src/embedding.rs`
- Modify: `crates/art-retrieval/src/lib.rs`
- Modify: `crates/art-retrieval/Cargo.toml`
- Modify: `Cargo.toml`
- Test: `crates/art-retrieval/tests/embedding_contracts.rs`

**Interfaces:**
- Produces: `EmbeddingEndpoint`, `EmbeddingProvider`, `EmbeddingInput`, `EmbeddingErrorKind`, and `ProviderFingerprint`.
- Consumes: endpoint JSON at `<ART_HOME>/config/art/embedding/default.json` or an explicit absolute path.

- [ ] **Step 1: Write failing configuration tests**

```rust
#[test]
fn endpoint_requires_generic_schema_and_owner_only_secret_file() {
    let endpoint = EmbeddingEndpoint::load(&fixture("endpoint.json")).unwrap();
    assert_eq!(endpoint.schema, "art.embedding.endpoint.v1");
    assert_eq!(endpoint.protocol, "openai_compatible");
    assert!(endpoint.dimensions > 0);
}

#[test]
fn endpoint_rejects_inline_tokens_and_unknown_fields() {
    assert_code(load_json(r#"{"schema":"art.embedding.endpoint.v1","token":"secret"}"#), "invalid_input");
}
```

Cover absolute HTTPS URL, model, optional revision, dimensions `1..=65_536`, normalization declaration, timeout `50..=30_000ms`, optional private CA path, owner-only regular token file, unknown fields, redirects, and secret-redacted errors.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p art-retrieval --test embedding_contracts endpoint -- --nocapture`

Expected: embedding types and parser are missing.

- [ ] **Step 3: Implement the minimal interface**

```rust
pub trait EmbeddingProvider: Send + Sync + std::fmt::Debug {
    fn fingerprint(&self) -> ProviderFingerprint;
    fn embed(&self, input: EmbeddingInput<'_>) -> ArtResult<Vec<Vec<f32>>>;
}

pub enum EmbeddingInput<'a> {
    Query(&'a str),
    Documents(&'a [String]),
}
```

Use `#[serde(deny_unknown_fields)]`; accept credentials only as file paths and validate Unix mode `0600` where supported.

- [ ] **Step 4: Verify parser GREEN**

Run: `cargo test -p art-retrieval --test embedding_contracts endpoint -- --nocapture`

Expected: strict valid fixtures pass and unsafe/malformed fixtures fail without echoing secret values.

- [ ] **Step 5: Commit the provider contract**

```bash
git add Cargo.toml Cargo.lock crates/art-retrieval/Cargo.toml crates/art-retrieval/src/embedding.rs crates/art-retrieval/src/lib.rs crates/art-retrieval/tests/embedding_contracts.rs
git commit -m "feat: define optional embedding provider contract"
```

### Task 2: Implement the OpenAI-compatible HTTPS adapter

**Files:**
- Modify: `crates/art-retrieval/src/embedding.rs`
- Test: `crates/art-retrieval/tests/embedding_contracts.rs`

**Interfaces:**
- Consumes: `EmbeddingEndpoint` and OpenAI-compatible `POST /v1/embeddings` JSON.
- Produces: `OpenAiCompatibleEmbeddingProvider::new` and bounded normalized vectors.

- [ ] **Step 1: Write failing protocol tests against a task-owned stub**

The stub records only request counts and returns deterministic vectors. Assert model/input encoding, query/document batches, response ordering by `index`, exact count/dimensions, finite values, maximum body size, TLS/custom-CA behavior where available, redirect rejection, timeout, 401, 429, 5xx, malformed JSON, and safe error text.

```rust
let vectors = provider.embed(EmbeddingInput::Query("private query")).unwrap();
assert_eq!(vectors, vec![vec![1.0, 0.0, 0.0]]);
assert_eq!(stub.request_count(), 1);
```

- [ ] **Step 2: Run protocol tests and verify RED**

Run: `cargo test -p art-retrieval --test embedding_contracts openai_compatible -- --nocapture`

Expected: the concrete adapter is missing.

- [ ] **Step 3: Implement bounded HTTPS calls**

Build a `reqwest::blocking::Client` with redirects disabled, configured timeout, rustls, optional private CA, `Content-Type: application/json`, and bearer token read at request time. Cap request documents at 32, each input at 64 KiB, and response at 16 MiB. Sort by response `index`, reject duplicates/gaps/non-finite values/wrong dimensions, and normalize only when configuration declares provider output unnormalized.

- [ ] **Step 4: Verify protocol GREEN and no secret output**

Run: `cargo test -p art-retrieval --test embedding_contracts openai_compatible -- --nocapture`

Expected: all success/failure cases pass and captured diagnostics contain no token or input body.

- [ ] **Step 5: Commit the HTTPS adapter**

```bash
git add crates/art-retrieval/src/embedding.rs crates/art-retrieval/tests/embedding_contracts.rs
git commit -m "feat: add openai compatible embedding adapter"
```

### Task 3: Add isolated semantic projections

**Files:**
- Create: `crates/art-retrieval/src/semantic.rs`
- Modify: `crates/art-retrieval/src/lib.rs`
- Test: `crates/art-retrieval/tests/semantic_projection_contracts.rs`

**Interfaces:**
- Produces: `SemanticProjection::open_private`, `open_shared`, `rebuild_private`, `rebuild_shared`, `rank`, and `diagnostics`.
- Consumes: canonical Agent memory/Knowledge Edition text, source epochs, and provider fingerprint.

- [ ] **Step 1: Write failing isolation and rebuild tests**

```rust
let private = SemanticProjection::open_private(&vault, fingerprint.clone()).unwrap();
let shared = SemanticProjection::open_shared(&knowledge, fingerprint.clone()).unwrap();
assert_ne!(private.path(), shared.path());
assert_eq!(private.rebuild_private(&vault, &provider).unwrap(), eligible_count);
assert_eq!(private.diagnostics().unwrap().source_epoch, vault.index_epoch().unwrap());
```

Cover distinct per-Agent paths, owner-only permissions, current Editions only, content-hash reuse, provider/epoch staleness, staged atomic replacement, interrupted rebuild preservation, corrupt BLOBs, dimensions, and exclusion from existing backup/export allowlists.

- [ ] **Step 2: Run projection tests and verify RED**

Run: `cargo test -p art-retrieval --test semantic_projection_contracts -- --nocapture`

Expected: semantic projection APIs are missing.

- [ ] **Step 3: Implement staged SQLite projections**

Use schema:

```sql
CREATE TABLE metadata (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE vectors (
  subject_ref TEXT PRIMARY KEY,
  content_sha256 TEXT NOT NULL,
  vector BLOB NOT NULL
);
```

Encode normalized finite vectors as little-endian `f32`; write to a task-attributable staging sibling, verify row counts and metadata, fsync/close, then atomically rename. Paths are:

```text
data/art/agents/<agent-id>/retrieval/semantic.sqlite3
data/art/knowledge-vault/.art/retrieval/semantic.sqlite3
```

- [ ] **Step 4: Implement deterministic cosine ranking**

Reject stale projections before reading rows. Scan the lane-local vectors, compute cosine similarity, order descending then `subject_ref` ascending, and return bounded ranked references without bodies.

- [ ] **Step 5: Verify projection GREEN and backup compatibility**

Run: `cargo test -p art-retrieval --test semantic_projection_contracts && cargo test -p art-knowledge --test backup_contracts && cargo test -p art-agent-store --test vault_contracts`

Expected: projection, backup, and private storage suites pass.

- [ ] **Step 6: Commit semantic projections**

```bash
git add crates/art-retrieval/src/semantic.rs crates/art-retrieval/src/lib.rs crates/art-retrieval/tests/semantic_projection_contracts.rs
git commit -m "feat: add isolated semantic projections"
```

### Task 4: Add semantic/hybrid orchestration and fail-open behavior

**Files:**
- Modify: `crates/art-retrieval/src/lib.rs`
- Modify: `crates/art-retrieval/src/ranking.rs`
- Test: `crates/art-retrieval/tests/recall_contracts.rs`
- Test: `crates/art-retrieval/tests/embedding_contracts.rs`

**Interfaces:**
- Consumes: `EmbeddingProvider`, semantic ranked references, lexical/full-scan candidates, and lane admission.
- Produces: effective semantic/hybrid results plus `vector_status=ready|unavailable|stale|degraded` and lexical fallback.

- [ ] **Step 1: Write failing mode/fallback tests**

```rust
let bundle = engine.recall(RecallRequest {
    mode: RetrievalMode::Hybrid,
    ..RecallRequest::new("different vocabulary")
}).unwrap();
assert_eq!(bundle.requested_mode, RetrievalMode::Hybrid);
assert_eq!(bundle.vector_status, "ready");
assert!(bundle.candidate_sources.contains(&"semantic".to_string()));
```

Add tests for semantic no-overlap results, exact-ID lexical dominance in hybrid, disputed/expired/revoked filtering, deterministic ties, missing config, stale epoch, timeout, 401/429, malformed vectors, and byte-for-byte unchanged lexical item ordering under fallback.

- [ ] **Step 2: Run recall tests and verify RED**

Run: `cargo test -p art-retrieval --test recall_contracts semantic hybrid fallback -- --nocapture`

Expected: semantic/hybrid requests still return unsupported/unavailable behavior.

- [ ] **Step 3: Implement lane-local semantic admission and fusion**

Load canonical records only after semantic references are returned; apply existing identity/lifecycle/current/revoked checks before adding them to visible candidates. Fuse lexical and semantic ranks using a configurable policy object whose defaults are versioned but not model-specific:

```rust
pub struct RankFusionPolicy {
    pub lexical_weight: f64,
    pub semantic_weight: f64,
    pub rrf_k: u32,
}
```

Do not tune defaults against query IDs, document IDs, datasets, or qrels.

- [ ] **Step 4: Implement exact lexical fallback**

On any optional semantic failure, execute the existing lexical request path, preserve item ordering/scores/reasons, set `effective_mode=lexical`, add only safe fallback/status diagnostics, and never include provider response bodies.

- [ ] **Step 5: Verify orchestration GREEN**

Run: `cargo test -p art-retrieval --test recall_contracts && cargo test -p art-retrieval --test embedding_contracts && cargo test -p art-retrieval --test semantic_projection_contracts`

Expected: all retrieval modes and failure classes pass without cross-lane leakage.

- [ ] **Step 6: Commit semantic/hybrid recall**

```bash
git add crates/art-retrieval/src/lib.rs crates/art-retrieval/src/ranking.rs crates/art-retrieval/tests/recall_contracts.rs crates/art-retrieval/tests/embedding_contracts.rs
git commit -m "feat: add optional semantic and hybrid recall"
```

### Task 5: Wire configuration, reindex, health, and conformance evidence

**Files:**
- Modify: `crates/art-cli/src/main.rs`
- Modify: `crates/art-mcp/src/lib.rs`
- Modify: `crates/art-cli/tests/cli_contracts.rs`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`
- Modify: `crates/art-cli/tests/stdio_mcp_e2e.rs`
- Modify: `scripts/benchmark_beir_retrieval.py`
- Modify: `scripts/run_beir_retrieval_benchmark.sh`
- Modify: `docs/testing-retrieval.md`
- Modify: `docs/operations.md`
- Modify: `docs/security-model.md`

**Interfaces:**
- Produces: endpoint discovery, `art reindex --vectors`, non-secret Doctor/health diagnostics, and optional provider-qualified BEIR output.
- Consumes: embedding endpoint, semantic projection diagnostics, and existing aggregate-only benchmark harness.

- [ ] **Step 1: Write failing CLI/MCP wiring tests**

Assert endpoint discovery under the explicit ART home, per-request mode override, lexical default despite endpoint presence, vector rebuild counts, stale diagnostics, stable six-tool schema, and no credentials/queries in Doctor/health output.

- [ ] **Step 2: Run wiring tests and verify RED**

Run: `cargo test -p art-cli --test cli_contracts vectors -- --nocapture && cargo test -p art-mcp --test mcp_contracts vector -- --nocapture && cargo test -p art-cli --test stdio_mcp_e2e hybrid -- --nocapture`

Expected: endpoint discovery/reindex/diagnostics are missing.

- [ ] **Step 3: Implement runtime wiring**

Discover `<ART_HOME>/config/art/embedding/default.json`; allow an explicit endpoint path only through operator CLI configuration; construct the provider/projections once per CLI/MCP runtime; add `--vectors` to reindex; keep ordinary `art recall` lexical unless mode is explicitly configured or requested.

- [ ] **Step 4: Extend the benchmark as optional conformance evidence**

Add `--mode lexical|full_scan|semantic|hybrid` and provider fingerprint to aggregate output. Keep datasets/qrels/metrics fixed, retain no raw runs, and never fail the ART core release solely because semantic metrics underperform. A named provider profile may publish its own pass/fail report.

- [ ] **Step 5: Run complete optional-adapter verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash scripts/open_source_check.sh
bash tests/scripts/independence-scan.sh
```

Expected: all commands exit 0 with no configured live provider required.

- [ ] **Step 6: Commit adapter wiring and docs**

```bash
git add crates scripts docs Cargo.toml Cargo.lock
git commit -m "feat: complete optional embedding adapter"
```
