# ART Remote Semantic Retrieval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fail-open hybrid BM25/Qwen retrieval to ART, prove its quality with paired public BEIR evaluation, and cut over the formal Codex/DSH installation only if it improves.

**Architecture:** A strict HTTPS embedding client feeds disposable, physically separated SQLite vector projections. Recall unions lexical and dense candidates per lane and applies weighted RRF only after existing governance admission; optional semantic failures return unchanged lexical results.

**Tech Stack:** Rust 1.98, reqwest/rustls, rusqlite, Qwen3-Embedding-0.6B HTTPS service, Python BEIR harness.

**Spec:** `docs/superpowers/specs/2026-08-31-art-remote-semantic-retrieval-design.md`

## Global Constraints

- Private memory remains physically isolated per Agent; another Agent's vectors are indistinguishable from missing data.
- Shared vectors contain only current, non-revoked published Knowledge Editions.
- Vectors and endpoint credentials are rebuildable/private runtime data and never enter public Git or knowledge backup Git.
- Model identity is exactly `Qwen/Qwen3-Embedding-0.6B`, revision `97b0c614be4d77ee51c0cef4e5f07c00f9eb65b3`, dimensions `512`.
- Lexical weight is `1.0`, dense weight is `0.7`, and RRF `k=60` for every dataset and query.
- Semantic failure degrades to the unchanged lexical path; it never fails recall.

---

### Task 1: Strict endpoint configuration and client

**Files:**
- Create: `crates/art-retrieval/src/embedding.rs`
- Modify: `Cargo.toml`
- Modify: `crates/art-retrieval/Cargo.toml`
- Test: `crates/art-retrieval/tests/embedding_contracts.rs`

**Interfaces:**
- Produces: `EmbeddingEndpoint::load(&Path) -> ArtResult<Self>` and `EmbeddingClient::embed(&[String], InputType) -> ArtResult<Vec<Vec<f32>>>`.

- [ ] Write tests for exact schema/model/revision/dimensions, owner-only regular token/CA files, no inline secret fields, TLS response shape, non-finite/oversized/wrong-revision rejection, and redacted errors.
- [ ] Run `cargo test -p art-retrieval --test embedding_contracts` and verify failures are caused by missing interfaces.
- [ ] Implement the minimal strict parser and bounded HTTPS client with redirects disabled and 650 ms total timeout.
- [ ] Run the focused test and the existing retrieval tests until green.
- [ ] Commit endpoint/client code and RED-to-GREEN evidence.

### Task 2: Isolated atomic vector projections

**Files:**
- Create: `crates/art-retrieval/src/vector_projection.rs`
- Modify: `crates/art-retrieval/src/lib.rs`
- Modify: `crates/art-knowledge/src/lib.rs`
- Test: `crates/art-retrieval/tests/vector_projection_contracts.rs`

**Interfaces:**
- Consumes: `EmbeddingClient::embed` and existing `AgentVault::list`, `AgentVault::index_epoch`, `KnowledgeVault::list_current`, `KnowledgeVault::index_epoch`.
- Produces: `VectorRuntime::open`, `rebuild_private`, `rebuild_knowledge`, `rank_private`, `rank_knowledge`, and projection diagnostics.

- [ ] Write tests proving separate per-Agent files, owner-only modes, only current shared Editions, provider/epoch staleness, exact vector dimensions, atomic replacement, and preservation of the prior complete file on failure.
- [ ] Run the focused test and verify the expected missing-projection failures.
- [ ] Add a read-only Knowledge Vault root accessor and implement staged SQLite projections with BLOB f32 vectors and deterministic cosine ordering.
- [ ] Run focused and storage/knowledge tests until green.
- [ ] Commit projection code and tests.

### Task 3: Governance-first hybrid ranking and fallback

**Files:**
- Modify: `crates/art-retrieval/src/ranking.rs`
- Modify: `crates/art-retrieval/src/lib.rs`
- Test: `crates/art-retrieval/tests/recall_contracts.rs`
- Test: `crates/art-retrieval/tests/embedding_contracts.rs`

**Interfaces:**
- Consumes: dense ranked subject references and existing BM25 ranked candidates.
- Produces: lane-local weighted RRF and `vector_status` values `unavailable|ready|stale|degraded`.

- [ ] Write tests for semantic no-overlap recall, lexical dominance for exact IDs, no cross-Agent leakage, disputed/expired/revoked filtering before fusion, deterministic ties, and identical lexical output under timeout/401/429/malformed response.
- [ ] Run focused tests and verify failures describe absent hybrid behavior.
- [ ] Implement union, admission, weighted RRF (`1.0/0.7`, `k=60`), safe cautions, and lexical fallback.
- [ ] Run all retrieval tests, including the ignored target-Mac performance contract, until green.
- [ ] Commit hybrid behavior and tests.

### Task 4: CLI, MCP, reindex, and Doctor wiring

**Files:**
- Modify: `crates/art-cli/src/main.rs`
- Modify: `crates/art-mcp/src/lib.rs`
- Modify: `crates/art-cli/tests/cli_contracts.rs`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`
- Modify: `docs/architecture.md`
- Modify: `docs/security-model.md`
- Modify: `docs/operations.md`

**Interfaces:**
- Produces: optional `embedding_endpoint` config, `<ART_HOME>/config/art/embedding/default.json` discovery, `art reindex --vectors`, MCP semantic recall, and non-secret Doctor diagnostics.

- [ ] Write failing CLI/MCP tests for discovery priority, unknown/inline-secret rejection, vector rebuild controls, stable six-tool schema, ready/degraded statuses, and safe Doctor JSON.
- [ ] Run focused tests and verify configuration/wiring failures.
- [ ] Implement runtime construction shared by CLI and MCP and update operator documentation.
- [ ] Run CLI, stdio MCP, and plugin-surface tests until green.
- [ ] Commit runtime wiring and documentation.

### Task 5: Paired BEIR experiment

**Files:**
- Modify: `scripts/benchmark_beir_retrieval.py`
- Modify: `scripts/run_beir_retrieval_benchmark.sh`
- Modify: `docs/testing-retrieval.md`
- Create after success: `docs/artifacts/retrieval-acceptance-2026-08-31-v0.3.0.md`

**Interfaces:**
- Produces: one aggregate JSON with lexical, hybrid, optional dense-control metrics, absolute/relative deltas, indexing duration, vector status, and latency.

- [ ] Write harness self-tests for paired fixture reuse, fixed query order, delta math, aggregate-only output, and failure when hybrid regresses.
- [ ] Run self-tests and verify paired mode is absent.
- [ ] Implement paired mode without changing datasets, qrels, metric definitions, or BM25 baseline behavior.
- [ ] Download/reuse verified task-owned BEIR fixtures, run SciFact and NFCorpus against the accepted endpoint, and retain only aggregate evidence.
- [ ] Compare Recall@10, nDCG@10, nDCG@3, MRR, empty queries, p50/p95/p99, and indexing time; stop release work if either dataset regresses.
- [ ] Commit the harness and non-sensitive aggregate report only if gates pass.

### Task 6: Formal release and installed E2E gate

**Files:**
- Modify after all gates pass: workspace versions, changelog, release metadata, plugin package, and release acceptance report.
- Modify local runtime only after the public release tag is verified.

**Interfaces:**
- Produces: signed/checksummed ART 0.3.0 release and exact-version local Codex/DSH installations configured for the MacBook endpoint.

- [ ] Run `cargo test --workspace --all-features`, open-source/security scans, release gate, install lifecycle, backup/restore, stress, and version consistency checks.
- [ ] Build candidate assets and prove they contain no endpoint credentials, private paths, vectors, memories, or knowledge bodies.
- [ ] Run candidate Codex and DSH E2E plus service restart/unreachable fallback in isolated homes.
- [ ] Publish only if all gates pass, then install the exact public artifact, create owner-only `default.json`, rebuild all formal Agent/shared projections, and restart consumers.
- [ ] Run real Codex recall, real DSH recall, 20/20 reconnect, Doctor, knowledge count/hash, private Agent isolation, and backup verification on final installed bytes.
- [ ] Remove task-owned datasets, generated Vaults, targets, staging files, and worktrees; report before/after storage and retained formal assets.
