# ART Retrieval Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore ART's underlying BM25 retrieval quality, add bounded result-depth controls, and make SciFact/NFCorpus quality a reproducible release gate.

**Architecture:** The private and knowledge stores return records with one-based lexical ranks from a broad FTS5 query. `art-retrieval` preserves that dominant rank through bounded exact/token boosts and renders a caller-requested, token-budget-limited result depth. CLI and MCP add optional compatible controls; a non-default BEIR harness evaluates the compiled product path in isolated ART homes.

**Tech Stack:** Rust 1.98, SQLite FTS5 through rusqlite 0.37, Jieba, clap, rmcp/schemars, serde, tempfile, BEIR JSONL/TSV fixtures.

**Spec:** `docs/superpowers/specs/2026-08-30-art-retrieval-quality-design.md`

## Global Constraints

- Keep private Agent Vaults physically separate and keep shared Knowledge Editions as a different governed object family.
- Preserve the exact six Agent-safe MCP tool names and process-bound Agent identity.
- Existing callers that omit result-depth fields receive at most three private and three knowledge results.
- Explicit result depths are integers in `1..=20` and remain constrained by the token budget.
- Do not add a model dependency, implicit download, vector schema, canonical object migration, or output-schema version change.
- Tests and benchmarks use explicit task-owned ART homes and never write to `~/.across`.
- Public corpora, generated Vaults, raw run files, queries, and qrels are never committed.
- Every production behavior is introduced by a test that is observed failing for the expected reason before implementation.

---

### Task 1: Preserve ranked BM25 candidates in both stores

**Files:**
- Modify: `crates/art-agent-store/src/lib.rs:25-35,415-440`
- Modify: `crates/art-agent-store/tests/vault_contracts.rs`
- Modify: `crates/art-knowledge/src/lib.rs:40-82,459-476`
- Modify: `crates/art-knowledge/tests/knowledge_contracts.rs`

**Interfaces:**
- Consumes: existing `fts_expression`, `MemoryArtifact`, `EditionRecord`, and store connection helpers.
- Produces: `RankedMemoryCandidate { artifact: MemoryArtifact, lexical_rank: usize }`, `RankedEditionCandidate { edition: EditionRecord, lexical_rank: usize }`, and `search_ranked_candidates(&[String], usize)` methods used by Task 2. Existing bare `search_candidates(&[String])` wrappers remain source-compatible.

- [ ] **Step 1: Write failing private-store ranking tests**

Add a test that inserts three active memories where a rare query term makes the expected BM25 order unambiguous, then requests two candidates and asserts IDs plus ranks:

```rust
let ranked = vault.search_ranked_candidates(&["rareterm common".into(), "rareterm".into(), "common".into()], 2).unwrap();
assert_eq!(ranked.len(), 2);
assert_eq!(ranked[0].artifact.id, rare_title_id);
assert_eq!(ranked[0].lexical_rank, 1);
assert_eq!(ranked[1].lexical_rank, 2);
```

Add a second assertion proving that a whole-query phrase hit does not prevent another OR-term candidate from being returned.

- [ ] **Step 2: Run the private-store tests and verify RED**

Run:

```bash
cargo test -p art-agent-store --test vault_contracts ranked_search -- --nocapture
```

Expected: compilation fails because `search_candidates` has no limit parameter and returns `MemoryArtifact` without `artifact` or `lexical_rank`.

- [ ] **Step 3: Implement ranked private candidates minimally**

Add:

```rust
#[derive(Debug, Clone)]
pub struct RankedMemoryCandidate {
    pub artifact: MemoryArtifact,
    pub lexical_rank: usize,
}
```

Change candidate search to execute `fts_expression(terms)` once, use `ORDER BY rank,a.updated_at DESC,a.id ASC LIMIT ?3`, enumerate rows from one, and wrap each artifact. Validate `limit` in `1..=2048`, returning `ArtError::InvalidInput` otherwise.

- [ ] **Step 4: Run private-store tests and verify GREEN**

Run:

```bash
cargo test -p art-agent-store --test vault_contracts -- --nocapture
```

Expected: all private Vault contracts pass.

- [ ] **Step 5: Write failing knowledge-store ranking tests**

Create three published Editions with distinct rare/common term frequencies. Assert the rare Edition is lexical rank one, the returned ranks are `[1, 2]`, revoked/current filtering still applies, and a whole-query phrase hit does not suppress a different OR-term Edition.

- [ ] **Step 6: Run the knowledge-store test and verify RED**

Run:

```bash
cargo test -p art-knowledge --test knowledge_contracts ranked_search -- --nocapture
```

Expected: compilation fails because ranked knowledge candidates and the limit parameter do not exist.

- [ ] **Step 7: Implement ranked knowledge candidates minimally**

Add:

```rust
#[derive(Debug, Clone)]
pub struct RankedEditionCandidate {
    pub edition: EditionRecord,
    pub lexical_rank: usize,
}
```

Execute the broad expression once with `ORDER BY rank,p.published_at DESC,p.edition_id ASC LIMIT ?2`, enumerate rows from one, read each current non-revoked Edition, and enforce `1..=2048`.

- [ ] **Step 8: Run both store suites and commit**

Run:

```bash
cargo test -p art-agent-store --test vault_contracts
cargo test -p art-knowledge --test knowledge_contracts
```

Expected: both suites pass with no failures.

Commit:

```bash
git add crates/art-agent-store crates/art-knowledge
git commit -m "feat: preserve ranked BM25 retrieval candidates"
```

### Task 2: Replace overlap-only ordering with bounded BM25-first fusion

**Files:**
- Create: `crates/art-retrieval/src/ranking.rs`
- Modify: `crates/art-retrieval/src/lib.rs:1-345`
- Modify: `crates/art-retrieval/tests/recall_contracts.rs`

**Interfaces:**
- Consumes: `RankedMemoryCandidate`, `RankedEditionCandidate`, `RecallItem`, normalized exact/token/bigram features.
- Produces: `rank_score(lexical_rank, exact, token_coverage, bigram_coverage) -> f64` and BM25-dominant ordered Recall items for Task 3.

- [ ] **Step 1: Write the failing BM25-dominance contract**

Seed enough documents that the current overlap ratio promotes a common-term document over the rare-term BM25 leader. Assert the rare-term leader remains first after `RecallEngine::recall` and includes `bm25_rank` in `match_reasons`:

```rust
let bundle = engine.recall(RecallRequest::new("rareterm common filler")).unwrap();
assert_eq!(bundle.knowledge_editions[0].subject_ref, format!("knowledge:{rare_id}"));
assert!(bundle.knowledge_editions[0].match_reasons.contains(&"bm25_rank".into()));
```

- [ ] **Step 2: Run the targeted retrieval test and verify RED**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts bm25_rank_dominates_overlap -- --nocapture
```

Expected: assertion fails because current code re-sorts solely by overlap score and has no `bm25_rank` reason.

- [ ] **Step 3: Implement the minimal ranked score**

Create `ranking.rs` with a query-local score whose BM25 component is dominant:

```rust
pub(crate) fn rank_score(
    lexical_rank: usize,
    exact: bool,
    token_coverage: f64,
    bigram_coverage: f64,
) -> f64 {
    let base = 1.0 / (60.0 + lexical_rank as f64);
    let bounded_bonus = if exact { 0.000_20 } else { 0.0 }
        + token_coverage.clamp(0.0, 1.0) * 0.000_04
        + bigram_coverage.clamp(0.0, 1.0) * 0.000_02;
    base + bounded_bonus
}
```

Replace bare candidate loops with ranked candidates, add `bm25_rank`, retain existing exact/Jieba/CJK reasons, and remove the overlap-only `sort_items` path. Use subject reference only as the deterministic final tie-breaker.

- [ ] **Step 4: Run the targeted test and verify GREEN**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts bm25_rank_dominates_overlap -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Write failing bounded-boost and compatibility tests**

Add tests proving an exact phrase can improve a nearby result but cannot jump more than five lexical positions, disputed/expired items remain excluded, candidate authority remains `0.75`, private and knowledge channels stay separate, and all 64 Chinese golden queries retain a hit in the first three.

- [ ] **Step 6: Run retrieval contracts and verify RED where behavior is missing**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts -- --nocapture
```

Expected: the new bounded-jump assertion fails until fusion ordering enforces the five-rank invariant.

- [ ] **Step 7: Implement bounded fusion and refactor**

Represent each intermediate item with its lexical rank and score. Before sorting, clamp any bonus-driven ordering so an item cannot overtake a candidate more than five ranks ahead. Keep security and eligibility checks before insertion into the final ranked list. Split query parsing/ranking from bundle rendering without changing public JSON fields.

- [ ] **Step 8: Run retrieval and store regression suites and commit**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts
cargo test -p art-agent-store --test vault_contracts
cargo test -p art-knowledge --test knowledge_contracts
```

Expected: all pass.

Commit:

```bash
git add crates/art-retrieval crates/art-agent-store crates/art-knowledge
git commit -m "feat: rank ART recall with BM25-first fusion"
```

### Task 3: Add compatible result-depth controls to library, CLI, and MCP

**Files:**
- Modify: `crates/art-retrieval/src/lib.rs:55-70,102-198`
- Modify: `crates/art-retrieval/tests/recall_contracts.rs`
- Modify: `crates/art-cli/src/main.rs:55-100,408-450`
- Modify: `crates/art-cli/tests/cli_contracts.rs`
- Modify: `crates/art-mcp/src/lib.rs:30-45,186-202`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`

**Interfaces:**
- Consumes: Task 2 ranked output.
- Produces: optional `max_private_results` and `max_knowledge_results` across `RecallRequest`, CLI flags, and `RecallInput`.

- [ ] **Step 1: Write failing library depth and validation tests**

Extend a fixture to contain at least twelve matching private and knowledge records. Assert defaults remain three, explicit depth ten returns ten at budget 6000, budget allocation can reduce the requested cap, and `0`/`21` return `ART_INVALID_INPUT`:

```rust
let ten = engine.recall(RecallRequest {
    budget_tokens: 6_000,
    max_private_results: Some(10),
    max_knowledge_results: Some(10),
    ..RecallRequest::new("shared marker")
}).unwrap();
assert_eq!(ten.knowledge_editions.len(), 10);
```

- [ ] **Step 2: Run the library tests and verify RED**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts configurable_result_depth -- --nocapture
```

Expected: compilation fails because the two fields do not exist.

- [ ] **Step 3: Implement library result depths minimally**

Add the two optional fields to `RecallRequest::new` with `None`. Validate each requested value is `1..=20`. Compute candidate depth as `512` for default requests and `min(2048, max(512, requested * 64))` for explicit requests. Compute effective output caps as the minimum of request/default and the existing private/knowledge token allocations.

- [ ] **Step 4: Run library tests and verify GREEN**

Run:

```bash
cargo test -p art-retrieval --test recall_contracts configurable_result_depth -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Write failing CLI contract tests**

Assert `art recall --help` exposes both flags, a task-owned 12-record Vault returns ten knowledge results with `--budget-tokens 6000 --max-knowledge-results 10`, omission preserves three, and invalid 21 emits `ART_INVALID_INPUT`.

- [ ] **Step 6: Run CLI tests and verify RED**

Run:

```bash
cargo test -p art-cli --test cli_contracts recall_result_depth -- --nocapture
```

Expected: the CLI rejects the unknown flags.

- [ ] **Step 7: Implement CLI flags and verify GREEN**

Add optional clap fields and forward them unchanged into `RecallRequest`.

Run:

```bash
cargo test -p art-cli --test cli_contracts recall_result_depth -- --nocapture
```

Expected: pass.

- [ ] **Step 8: Write failing MCP schema and behavior tests**

Assert the existing `art_recall` tool schema includes optional integer fields with minimum 1 and maximum 20, omitted values retain three, explicit ten returns ten, and invalid values return stable invalid-input JSON-RPC errors.

- [ ] **Step 9: Run MCP tests and verify RED**

Run:

```bash
cargo test -p art-mcp --test mcp_contracts recall_result_depth -- --nocapture
```

Expected: schema and behavior assertions fail because the fields do not exist.

- [ ] **Step 10: Implement MCP fields, run all interface suites, and commit**

Add optional fields with schemars range annotations and forward them into `RecallRequest`.

Run:

```bash
cargo test -p art-retrieval --test recall_contracts
cargo test -p art-cli --test cli_contracts
cargo test -p art-cli --test stdio_mcp_e2e
cargo test -p art-mcp --test mcp_contracts
```

Expected: all pass and the tool count remains six.

Commit:

```bash
git add crates/art-retrieval crates/art-cli crates/art-mcp
git commit -m "feat: add bounded ART recall result depth"
```

### Task 4: Check in a reproducible BEIR product-path release gate

**Files:**
- Create: `scripts/benchmark_beir_retrieval.py`
- Create: `scripts/download_beir_retrieval_fixtures.sh`
- Create: `scripts/run_beir_retrieval_benchmark.sh`
- Create: `docs/testing-retrieval.md`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: compiled `art` CLI with Task 3 flags and operator-provided fixture root.
- Produces: aggregate JSON with dataset counts, Recall/MRR/nDCG/Accuracy at 1/3/10, latency percentiles, ART version, and pass/fail gates.

- [x] **Step 1: Write the product-path benchmark contract and observe baseline failure**

Implement a standalone, standard-library benchmark harness that accepts an
explicit dataset root, candidate binary, and new result path. The fixture
download verifies:

```text
scifact.zip   md5 5f7d1de60b170fc8027bb7898e2efca1
nfcorpus.zip  md5 a89dba18a62ef92f7d323ec890a0d38d
```

It must parse `corpus.jsonl`, `queries.jsonl`, and `qrels/test.tsv`, construct
only a temporary Knowledge Vault, invoke the selected compiled product path
with top ten, and assert the spec thresholds. The previously recorded installed
0.1.1 baseline supplies the RED comparison; the candidate runner supplies GREEN.

Run:

```bash
bash scripts/run_beir_retrieval_benchmark.sh /private/tmp/art-beir-fixtures /private/tmp/art-beir-results.json
```

Expected: the old installed binary cannot supply ten results; after Task 3, the
candidate completes the full metric assertions.

- [x] **Step 2: Add safe fixture download and benchmark runner scripts**

The download script accepts an explicit new output directory, downloads only the two official BEIR archives, verifies the exact MD5 values before extraction, and refuses symlinks/existing non-empty targets. The runner requires an explicit fixture root and output JSON path, sets a task-owned temporary ART home, and never defaults to `~/.across`.

- [x] **Step 3: Add documentation and repository exclusions**

Document dataset licensing responsibility, exact commands, metric formulas, expected counts, thresholds, isolation, and cleanup. Ignore `datasets/beir/`, `benchmarks/output/`, generated Vaults, and raw TREC run files.

- [x] **Step 4: Run the final benchmark and verify GREEN**

Run:

```bash
bash scripts/run_beir_retrieval_benchmark.sh /private/tmp/art-beir-fixtures /private/tmp/art-beir-results.json
```

Expected: SciFact Recall@10 >= 0.76 and nDCG@10 >= 0.64; NFCorpus Recall@10 >= 0.14 and nDCG@10 >= 0.29; all @3 metrics meet the non-regression gates.

- [x] **Step 5: Verify no corpus or private data is staged and commit**

Run:

```bash
git status --short
git ls-files | rg '(^|/)(corpus\.jsonl|queries\.jsonl|qrels|\.run\.trec|art-control\.sqlite3|agent-vault\.sqlite3)$' && exit 1 || true
```

Expected: only harness, scripts, documentation, and ignore rules are changed; no dataset or Vault file is tracked.

Commit:

```bash
git add .gitignore scripts/benchmark_beir_retrieval.py scripts/download_beir_retrieval_fixtures.sh scripts/run_beir_retrieval_benchmark.sh docs/testing-retrieval.md
git commit -m "test: gate ART retrieval quality with BEIR"
```

### Task 5: Full regression, performance, and release-candidate evidence

**Files:**
- Modify: `docs/testing.md`
- Create: `docs/artifacts/retrieval-acceptance-2026-08-30.md`

**Interfaces:**
- Consumes: Tasks 1-4 and their aggregate benchmark JSON.
- Produces: a non-sensitive acceptance report and a reviewable feature branch; formal installation remains unchanged.

- [x] **Step 1: Run formatting, lint, and complete workspace tests**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: zero failures and zero warnings; the large performance test remains the only ignored test in the normal suite.

- [x] **Step 2: Run the 10k/5k performance contract**

Run:

```bash
cargo test -p art-retrieval --test performance_contracts --release -- --ignored --nocapture
```

Expected: startup < 500 ms, cold recall < 150 ms, recall p95 < 150 ms in-process, capture p95 < 100 ms, and eight concurrent recalls each < 500 ms.

- [x] **Step 3: Run security and release gates**

Run:

```bash
bash scripts/open_source_check.sh
bash tests/scripts/release-gate.sh
```

Expected: both exit zero; no credentials, absolute developer paths, benchmark corpora, Vaults, or test-only data enter release payloads.

- [x] **Step 4: Re-run BEIR and record aggregate evidence**

Run the Task 4 release command from a clean fixture and record only aggregate metrics, latency, dataset checksums, ART commit, and commands in `docs/artifacts/retrieval-acceptance-2026-08-30.md`. Do not copy queries, qrels, document bodies, machine-private paths, or formal knowledge content.

- [x] **Step 5: Review repository state and commit evidence**

Run:

```bash
git diff --check
git status --short --branch
git log --oneline origin/main..HEAD
```

Expected: only intended source, tests, scripts, docs, and aggregate evidence differ from `origin/main`.

Commit:

```bash
git add docs/testing.md docs/artifacts/retrieval-acceptance-2026-08-30.md tests/scripts/test_migration.sh docs/superpowers/plans/2026-08-30-art-retrieval-quality.md
git commit -m "docs: record ART retrieval quality acceptance"
```

- [ ] **Step 6: Stop before formal installation**

Report source, benchmark, performance, packaging, and branch state. Do not replace the installed ART binary, restart Codex/DSH MCP servers, publish a release, push a branch, or change formal ART data until the operator reviews the completed candidate.
