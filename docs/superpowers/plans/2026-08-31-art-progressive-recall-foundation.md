# ART Progressive Recall Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic route, lexical, and governance-filtered full-scan recall paths while preserving ART's private/shared authority boundary and six-tool MCP contract.

**Architecture:** `art-retrieval` owns additive request/result policy and progressive orchestration. `art-agent-store` and `art-knowledge` own lane-local navigation projections beside their existing FTS projections. Existing clients default to the ART 0.2 lexical path; route and full-scan behavior are explicit and rebuildable.

**Tech Stack:** Rust 1.98, serde/schemars, rusqlite/FTS5, clap, rmcp, tempfile

**Spec:** `docs/superpowers/specs/2026-08-31-art-progressive-recall-architecture-design.md`

## Global Constraints

- Private memory remains physically isolated per Agent and MCP identity remains process-bound.
- Shared knowledge remains human-reviewed, immutable, and separately stored.
- The six MCP tool names remain unchanged.
- Existing callers that omit new fields receive the ART 0.2 lexical result path.
- Admission precedes ranking and candidate visibility in every mode.
- Tests use explicit task-owned `ART_HOME` roots and never touch formal Codex/DSH state.
- No full transcripts, secrets, recalled bundle bodies, or private knowledge enter Git or test evidence.

---

### Task 1: Add progressive recall contracts

**Files:**
- Create: `crates/art-retrieval/src/policy.rs`
- Modify: `crates/art-retrieval/src/lib.rs:24-78`
- Modify: `crates/art-mcp/src/lib.rs:24-45`
- Modify: `crates/art-cli/src/main.rs:68-110`
- Test: `crates/art-retrieval/tests/recall_contracts.rs`
- Test: `crates/art-mcp/tests/mcp_contracts.rs`
- Test: `crates/art-cli/tests/cli_contracts.rs`

**Interfaces:**
- Produces: `RetrievalMode::{Lexical,FullScan,Semantic,Hybrid}`, `RecallDetail::{Route,Recall}`, `RecallRequest.mode`, `RecallRequest.detail`, and additive Recall Bundle diagnostics.
- Consumes: existing `RecallRequest`, `RecallBundle`, CLI `recall`, and MCP `RecallInput` surfaces.

- [ ] **Step 1: Write failing library contract tests**

```rust
#[test]
fn recall_defaults_preserve_v020_lexical_behavior() {
    let request = RecallRequest::new("thunderbolt recovery");
    assert_eq!(request.mode, RetrievalMode::Lexical);
    assert_eq!(request.detail, RecallDetail::Recall);
}

#[test]
fn retrieval_modes_round_trip_as_stable_snake_case() {
    for (mode, json) in [
        (RetrievalMode::Lexical, r#""lexical""#),
        (RetrievalMode::FullScan, r#""full_scan""#),
        (RetrievalMode::Semantic, r#""semantic""#),
        (RetrievalMode::Hybrid, r#""hybrid""#),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), json);
    }
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p art-retrieval --test recall_contracts recall_defaults_preserve_v020_lexical_behavior retrieval_modes_round_trip_as_stable_snake_case -- --nocapture`

Expected: compilation fails because `RetrievalMode`, `RecallDetail`, and the new fields do not exist.

- [ ] **Step 3: Implement the minimal public policy types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode { Lexical, FullScan, Semantic, Hybrid }

impl Default for RetrievalMode {
    fn default() -> Self { Self::Lexical }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallDetail { Route, Recall }

impl Default for RecallDetail {
    fn default() -> Self { Self::Recall }
}
```

Add `mode` and `detail` to requests with serde defaults. Add `requested_mode`, `effective_mode`, `detail`, `map_status`, `candidate_sources`, and `fallback_reason` to the bundle. Keep `vector_status`.

- [ ] **Step 4: Add failing CLI and MCP schema tests**

```rust
assert!(schema.contains("full_scan"));
assert!(schema.contains("semantic"));
assert!(schema.contains("hybrid"));
assert_eq!(server.tool_names().len(), 6);
```

CLI test: invoke `art recall query --agent codex-main --mode full-scan --detail route --json` and assert parsing reaches the runtime rather than clap rejection.

- [ ] **Step 5: Wire additive CLI/MCP fields and verify GREEN**

Run: `cargo test -p art-retrieval --test recall_contracts && cargo test -p art-mcp --test mcp_contracts && cargo test -p art-cli --test cli_contracts`

Expected: all focused contract suites pass and the MCP tool count remains six.

- [ ] **Step 6: Commit the contract slice**

```bash
git add crates/art-retrieval/src/policy.rs crates/art-retrieval/src/lib.rs crates/art-mcp/src/lib.rs crates/art-cli/src/main.rs crates/art-retrieval/tests/recall_contracts.rs crates/art-mcp/tests/mcp_contracts.rs crates/art-cli/tests/cli_contracts.rs
git commit -m "feat: add progressive recall policy contracts"
```

### Task 2: Add governance-filtered full scan

**Files:**
- Modify: `crates/art-retrieval/src/lib.rs:109-248`
- Test: `crates/art-retrieval/tests/recall_contracts.rs`
- Test: `crates/art-retrieval/tests/performance_contracts.rs`

**Interfaces:**
- Consumes: `AgentVault::list`, `KnowledgeVault::list_current`, `RetrievalMode::FullScan`, and existing admission/ranking helpers.
- Produces: complete lane-local candidate enumeration with `effective_mode=full_scan` and `candidate_sources=["canonical_full_scan"]`.

- [ ] **Step 1: Write failing full-scan behavior tests**

Create more than 2,048 eligible records, place the only relevant record after the lexical candidate window, then assert:

```rust
let bundle = engine.recall(RecallRequest {
    mode: RetrievalMode::FullScan,
    ..RecallRequest::new("rare exact recovery phrase")
}).unwrap();
assert_eq!(bundle.effective_mode, RetrievalMode::FullScan);
assert_eq!(bundle.private_memories[0].subject_ref, expected_ref);
assert_eq!(bundle.candidate_sources, vec!["canonical_full_scan"]);
```

Add separate tests proving disputed/expired private records and revoked/non-current Editions are absent even in full-scan mode.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p art-retrieval --test recall_contracts full_scan -- --nocapture`

Expected: the engine still follows the FTS candidate path and cannot report full scan.

- [ ] **Step 3: Implement lane-local candidate enumeration**

Extract `recall_private_candidates` and `recall_knowledge_candidates`. For full scan, use canonical `list()`/`list_current()`, apply admission before calling the lexical scorer, and preserve deterministic ties by `subject_ref`.

```rust
match request.mode {
    RetrievalMode::FullScan => private_vault.list()?,
    _ => private_vault.search_ranked_candidates(&terms, limit)?
        .into_iter().map(|item| item.artifact).collect(),
}
```

- [ ] **Step 4: Verify focused and performance GREEN**

Run: `cargo test -p art-retrieval --test recall_contracts && cargo test -p art-retrieval --test performance_contracts`

Expected: full scan finds the tail record, governance exclusions hold, and existing lexical latency contracts remain unchanged.

- [ ] **Step 5: Commit full scan**

```bash
git add crates/art-retrieval/src/lib.rs crates/art-retrieval/tests/recall_contracts.rs crates/art-retrieval/tests/performance_contracts.rs
git commit -m "feat: add governed full scan recall"
```

### Task 3: Add deterministic lane-local navigation projections

**Files:**
- Modify: `crates/art-agent-store/src/lib.rs`
- Modify: `crates/art-knowledge/src/lib.rs`
- Test: `crates/art-agent-store/tests/vault_contracts.rs`
- Test: `crates/art-knowledge/tests/knowledge_contracts.rs`

**Interfaces:**
- Produces: `MemoryNavigationEntry`, `KnowledgeNavigationEntry`, `AgentVault::navigation_entries`, `AgentVault::rebuild_navigation`, `KnowledgeVault::navigation_entries`, and `KnowledgeVault::rebuild_navigation`.
- Consumes: canonical memory revisions, current Edition projections, feedback receipts, and lane index epochs.

- [ ] **Step 1: Write failing private navigation tests**

```rust
let rebuilt = vault.rebuild_navigation().unwrap();
assert_eq!(rebuilt, 2);
let entries = vault.navigation_entries().unwrap();
assert!(entries.iter().all(|item| item.agent_id == agent));
assert_eq!(entries[0].source_epoch, vault.index_epoch().unwrap());
```

Assert capture/update/archive refreshes or invalidates the projection and another Agent Vault cannot observe the entries.

- [ ] **Step 2: Write failing shared catalog tests**

```rust
let rebuilt = knowledge.rebuild_navigation().unwrap();
assert_eq!(rebuilt, knowledge.list_current().unwrap().len() as u64);
assert!(knowledge.navigation_entries().unwrap().iter().all(|item| item.current));
```

Assert replacement and revocation remove old Editions from the current catalog without rewriting Edition files.

- [ ] **Step 3: Run storage tests and verify RED**

Run: `cargo test -p art-agent-store --test vault_contracts navigation -- --nocapture && cargo test -p art-knowledge --test knowledge_contracts navigation -- --nocapture`

Expected: navigation types and methods are missing.

- [ ] **Step 4: Implement projection tables and rebuilds**

Add idempotent tables:

```sql
CREATE TABLE IF NOT EXISTS memory_navigation (
  memory_id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  scope_type TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  revision INTEGER NOT NULL,
  updated_at TEXT NOT NULL,
  usage_count INTEGER NOT NULL DEFAULT 0,
  source_epoch TEXT NOT NULL
);
```

```sql
CREATE TABLE IF NOT EXISTS knowledge_navigation (
  edition_id TEXT PRIMARY KEY,
  knowledge_key TEXT NOT NULL,
  edition_number INTEGER NOT NULL,
  title TEXT NOT NULL,
  applicability TEXT NOT NULL,
  published_at TEXT NOT NULL,
  current INTEGER NOT NULL,
  usage_count INTEGER NOT NULL DEFAULT 0,
  source_epoch TEXT NOT NULL
);
```

Rebuild each table transactionally from canonical records, replacing the previous rows only after the new rows are complete. Extract applicability from the immutable Edition Markdown `## Applicability` section.

- [ ] **Step 5: Wire lifecycle maintenance and verify GREEN**

Update capture/revision/status/feedback transactions and publish/revoke/reconcile transactions so navigation rows cannot appear ahead of canonical commits. Re-run:

`cargo test -p art-agent-store --test vault_contracts && cargo test -p art-knowledge --test knowledge_contracts`

Expected: all storage, corruption, revocation, recovery, and navigation tests pass.

- [ ] **Step 6: Commit navigation storage**

```bash
git add crates/art-agent-store/src/lib.rs crates/art-agent-store/tests/vault_contracts.rs crates/art-knowledge/src/lib.rs crates/art-knowledge/tests/knowledge_contracts.rs
git commit -m "feat: add lane-local recall navigation projections"
```

### Task 4: Add route-level progressive recall

**Files:**
- Create: `crates/art-retrieval/src/navigation.rs`
- Modify: `crates/art-retrieval/src/lib.rs`
- Test: `crates/art-retrieval/tests/recall_contracts.rs`

**Interfaces:**
- Consumes: lane-local navigation entries and `RecallDetail::Route`.
- Produces: `NavigationTopic`, `RecallBundle.navigation_topics`, `map_status=ready|stale|degraded`, and zero body excerpts in route mode.

- [ ] **Step 1: Write failing route tests**

```rust
let bundle = engine.recall(RecallRequest {
    detail: RecallDetail::Route,
    ..RecallRequest::new("release recovery")
}).unwrap();
assert!(bundle.private_memories.is_empty());
assert!(bundle.knowledge_editions.is_empty());
assert!(!bundle.navigation_topics.is_empty());
assert!(bundle.navigation_topics.iter().all(|topic| topic.subject_refs.len() <= 8));
```

Add tests for bounded topics, stable ordering, no bodies, no private cross-Agent identifiers, and stale-map fallback.

- [ ] **Step 2: Run route tests and verify RED**

Run: `cargo test -p art-retrieval --test recall_contracts route -- --nocapture`

Expected: route detail is parsed but the bundle still contains recall excerpts and no navigation topics.

- [ ] **Step 3: Implement deterministic topic routing**

Normalize query/title/scope/applicability terms using the existing Unicode/Jieba helpers. Group by `(lane, scope_key|knowledge_key)`, rank exact then token/bigram coverage then usage/recency, cap at 12 topics and 8 subject refs per topic, and return only titles/scopes/counts/refs.

- [ ] **Step 4: Verify route and lexical compatibility GREEN**

Run: `cargo test -p art-retrieval --test recall_contracts`

Expected: route tests pass and default lexical output fixtures remain unchanged except additive diagnostics.

- [ ] **Step 5: Commit route recall**

```bash
git add crates/art-retrieval/src/navigation.rs crates/art-retrieval/src/lib.rs crates/art-retrieval/tests/recall_contracts.rs
git commit -m "feat: add progressive route recall"
```

### Task 5: Wire reindex, health, integrations, and documentation

**Files:**
- Modify: `crates/art-cli/src/main.rs`
- Modify: `crates/art-mcp/src/lib.rs`
- Modify: `crates/art-cli/tests/cli_contracts.rs`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`
- Modify: `crates/art-cli/tests/stdio_mcp_e2e.rs`
- Modify: `docs/architecture.md`
- Modify: `docs/memory-and-knowledge.md`
- Modify: `docs/operations.md`
- Modify: `docs/testing-retrieval.md`
- Modify: `integrations/codex/art-recall/SKILL.md`
- Modify: `integrations/dsh/art-recall/SKILL.md`
- Modify: `plugin/agent-recall-trail/skills/agent-recall-trail/SKILL.md`

**Interfaces:**
- Consumes: navigation rebuild/read APIs and progressive recall fields.
- Produces: `art reindex --navigation`, bounded health diagnostics, and host guidance for route→recall→read.

- [ ] **Step 1: Write failing CLI/MCP integration tests**

```rust
assert_eq!(doctor["agent_vault"]["navigation_aligned"], true);
assert_eq!(doctor["knowledge_vault"]["navigation_aligned"], true);
assert_eq!(health["map_status"], "ready");
```

Add a stdio journey that calls route, then recall, then exact read and confirms all three stay bound to one Agent.

- [ ] **Step 2: Run integration tests and verify RED**

Run: `cargo test -p art-cli --test cli_contracts reindex_navigation -- --nocapture && cargo test -p art-cli --test stdio_mcp_e2e progressive -- --nocapture && cargo test -p art-mcp --test mcp_contracts health -- --nocapture`

Expected: flags and diagnostics are missing.

- [ ] **Step 3: Implement CLI/MCP wiring**

Extend `Reindex` with `--navigation`; include alignment/count/status without paths or content in Doctor/health; pass `mode` and `detail` from CLI/MCP to `RecallRequest`.

- [ ] **Step 4: Update all user-facing guidance together**

Document the five logical layers, four modes, route→recall→read flow, lexical default, full-scan semantics, optional embedding boundary, and unchanged human knowledge governance. Remove any statement that ART 0.3 requires a fixed or remote model.

- [ ] **Step 5: Verify the complete foundation slice**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash scripts/open_source_check.sh
bash tests/scripts/independence-scan.sh
```

Expected: all commands exit 0; no test writes outside task-owned homes.

- [ ] **Step 6: Commit the progressive foundation**

```bash
git add crates docs integrations plugin
git commit -m "feat: complete progressive recall foundation"
```
