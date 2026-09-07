# ART MCP Elicitation Governance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship ART 0.3.2 with a seventh Agent-safe MCP tool that requests human Knowledge Proposal review and publication through capability-negotiated MCP form Elicitation.

**Architecture:** `art-mcp` owns capability negotiation and user interaction, while `art-knowledge` remains the sole review/publication state owner. The Agent supplies only an operation and exact Proposal reference; decision, reason, and publication confirmation come exclusively from the MCP client's Elicitation response, with CLI fallback when the client lacks the capability.

**Tech Stack:** Rust 1.98, rmcp 3.1.4 with `elicitation`, serde/schemars, rusqlite, Tokio duplex MCP integration tests, Python stdio launch tests, isolated Codex CLI acceptance.

**Spec:** `docs/superpowers/specs/2026-09-06-art-mcp-elicitation-governance-design.md`

## Global Constraints

- The MCP surface changes intentionally from exactly six to exactly seven tools.
- Agents may request governance but may not submit a decision, reason, actor, or publication confirmation.
- Review and publication are separate tool calls and separate Elicitations.
- Unsupported, declined, cancelled, timed-out, malformed, stale, or disconnected flows perform zero governance writes.
- Existing elevated/high-risk independent-human rules remain unchanged and cannot be satisfied by changing client metadata.
- Existing CLI review and publication remain supported.
- Tests use a task-owned explicit `ART_HOME`; automated tests never touch formal `~/.across` state.
- Existing private memory, proposal, Edition, backup, migration, and retrieval formats remain unchanged.

---

### Task 1: Lock the seven-tool and no-Agent-decision contracts

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`
- Test: `crates/art-mcp/tests/mcp_contracts.rs`

**Interfaces:**
- Consumes: existing `ArtMcpServer::tool_names()` and `tool_schema_json()`.
- Produces: the exact tool name `art_knowledge_governance` and a strict `KnowledgeGovernanceInput { operation, proposal_id, revision }` schema.

- [ ] **Step 1: Write the failing surface test**

Change the expected tool set to include `art_knowledge_governance`, rename the test to `tool_surface_is_exactly_seven_agent_safe_tools`, and inspect the generated schema:

```rust
let schema: Value = serde_json::from_str(&server.tool_schema_json()).unwrap();
let governance = schema.as_array().unwrap().iter()
    .find(|tool| tool["name"] == "art_knowledge_governance")
    .unwrap();
let properties = &governance["inputSchema"]["properties"];
assert!(properties.get("operation").is_some());
assert!(properties.get("proposal_id").is_some());
assert!(properties.get("revision").is_some());
for forbidden in ["decision", "reason", "actor", "confirm"] {
    assert!(properties.get(forbidden).is_none());
}
assert_eq!(governance["inputSchema"]["additionalProperties"], false);
```

- [ ] **Step 2: Run the test and observe the expected failure**

Run: `cargo test -p art-mcp --test mcp_contracts tool_surface_is_exactly_seven_agent_safe_tools --locked`

Expected: FAIL because only six tools exist and `art_knowledge_governance` is absent.

- [ ] **Step 3: Enable the rmcp Elicitation feature and add only the input types**

Update the workspace dependency:

```toml
rmcp = { version = "3.1.4", features = ["server", "transport-io", "macros", "schemars", "elicitation"] }
```

Add strict types in `crates/art-mcp/src/lib.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeGovernanceOperation { Review, Publish }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGovernanceInput {
    pub operation: KnowledgeGovernanceOperation,
    pub proposal_id: String,
    pub revision: u32,
}
```

Register a temporary tool handler that returns `ART_NOT_IMPLEMENTED` without mutating state. Its description must state that the Agent requests but cannot supply the human decision.

- [ ] **Step 4: Run the surface test and verify it passes**

Run: `cargo test -p art-mcp --test mcp_contracts tool_surface_is_exactly_seven_agent_safe_tools --locked`

Expected: PASS, including forbidden-field and `additionalProperties=false` assertions.

- [ ] **Step 5: Commit the contract**

```bash
git add Cargo.toml Cargo.lock crates/art-mcp/src/lib.rs crates/art-mcp/tests/mcp_contracts.rs
git commit -m "test: lock MCP governance surface"
```

### Task 2: Implement form-capable review Elicitation

**Files:**
- Modify: `crates/art-mcp/src/lib.rs`
- Create: `crates/art-mcp/tests/elicitation_contracts.rs`
- Modify: `crates/art-knowledge/src/lib.rs`
- Test: `crates/art-mcp/tests/elicitation_contracts.rs`

**Interfaces:**
- Consumes: `KnowledgeVault::proposal`, `KnowledgeVault::review`, `ReviewActor::Human`, and `Peer<RoleServer>::elicit_with_timeout`.
- Produces: `ArtMcpServer::art_knowledge_governance(Parameters<KnowledgeGovernanceInput>, Peer<RoleServer>)` for `operation=review`, plus deterministic Proposal snapshots and human actor derivation.

- [ ] **Step 1: Add a paired rmcp client/server test harness**

Create a Tokio duplex harness whose `ClientHandler::create_elicitation` records the request and returns a queued `ElicitResult`. Its client capabilities must use:

```rust
ClientInfo::new(
    ClientCapabilities::builder().enable_elicitation().build(),
    Implementation::new("art-test-reviewer", "1"),
)
```

The harness must initialize a task-owned Agent, capture one sourced private memory, create one normal-risk Proposal through MCP, and call the governance tool through the protocol rather than invoking its Rust method directly.

- [ ] **Step 2: Write failing review tests**

Cover:

```rust
// accept + approve + non-empty reason => ProposalStatus::Approved
// accept + request_changes => ProposalStatus::ChangesRequested
// accept + reject => ProposalStatus::Rejected
// Elicitation message contains proposal id, revision, title, complete markdown,
// source_set_hash, and draft_hash.
// Recorded actor is generated by ART and cannot equal form content.
```

Run: `cargo test -p art-mcp --test elicitation_contracts review --locked`

Expected: FAIL because the temporary handler does not issue Elicitation.

- [ ] **Step 3: Add review response and snapshot types**

Implement private types equivalent to:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct ReviewElicitationResponse {
    decision: ReviewDecision,
    reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReviewDecision { Approve, RequestChanges, Reject }

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProposalSnapshot {
    id: String,
    revision: u32,
    status: ProposalStatus,
    source_set_hash: String,
    draft_hash: String,
}
```

Mark `ReviewElicitationResponse` as `rmcp::ElicitationSafe`. Bound reasons at runtime to 1..1000 Unicode scalar values, trim only surrounding whitespace, and never retain Elicitation metadata or raw client output.

- [ ] **Step 4: Implement review capability negotiation and commit**

Inside the tool handler:

```rust
if !peer.supported_elicitation_modes().contains(&ElicitationMode::Form) {
    return operator_action_required("ELICITATION_UNSUPPORTED", &snapshot);
}
let response = peer.elicit_with_timeout::<ReviewElicitationResponse>(
    review_message(&proposal, &snapshot),
    Some(Duration::from_secs(300)),
).await;
```

Map accept/decline/cancel/timeout without inventing consent. Re-read the exact Proposal and compare the complete snapshot before calling `KnowledgeVault::review`. Derive a bounded actor such as `local-elicitation:<16 hex chars>` from canonicalized ART host binding plus client implementation name/version; never use form content.

- [ ] **Step 5: Run review tests and the knowledge contracts**

Run:

```bash
cargo test -p art-mcp --test elicitation_contracts review --locked
cargo test -p art-knowledge --test knowledge_contracts --locked
```

Expected: PASS with no changes to existing knowledge-domain behavior.

- [ ] **Step 6: Commit review Elicitation**

```bash
git add crates/art-mcp/src/lib.rs crates/art-mcp/tests/elicitation_contracts.rs crates/art-knowledge/src/lib.rs
git commit -m "feat: review knowledge through MCP elicitation"
```

### Task 3: Implement separate publication Elicitation and fail-closed paths

**Files:**
- Modify: `crates/art-mcp/src/lib.rs`
- Modify: `crates/art-mcp/tests/elicitation_contracts.rs`
- Test: `crates/art-mcp/tests/elicitation_contracts.rs`

**Interfaces:**
- Consumes: the Task 2 snapshot helpers and `KnowledgeVault::publish(id, revision, true)`.
- Produces: `operation=publish`, distinct publication confirmation, stable no-write outcomes, and replay protection through Proposal state.

- [ ] **Step 1: Write failing publication tests**

Add protocol tests proving:

```rust
// review approve and publish use two different elicitation/create requests.
// accept + confirm=true publishes one immutable Edition.
// confirm=false, decline, cancel, malformed content, unsupported capability,
// blank/oversized review reason, wrong revision, and stale snapshot publish none.
// replay after publication cannot create a second Edition.
// elevated/high-risk second approval returns operator_action_required.
```

Run: `cargo test -p art-mcp --test elicitation_contracts --locked`

Expected: FAIL on publication and negative-path assertions.

- [ ] **Step 2: Add the publication form**

Define and mark safe:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct PublishElicitationResponse { confirm: bool }
```

The publication message must include Proposal ID, revision, knowledge key,
title, sensitivity, source-set hash, draft hash, and the immutable/shared
effect. It must not reuse the review response or treat review approval as
publication confirmation.

- [ ] **Step 3: Implement publication and stable outcome mapping**

After `accept + confirm=true`, re-read and compare the snapshot, require
`ProposalStatus::Approved`, then call the existing atomic publish path. Return
`outcome=published` with Edition ID, number, knowledge key, and manifest/body
hashes. For every non-consent path, return a bounded `art.mcp.v1` object and
leave the Proposal/Edition counts unchanged.

- [ ] **Step 4: Run the complete Elicitation and MCP suites**

Run:

```bash
cargo test -p art-mcp --test elicitation_contracts --locked
cargo test -p art-mcp --test mcp_contracts --locked
cargo test -p art-cli --test stdio_mcp_e2e --locked
```

Expected: PASS, including clean stdio shutdown and original six-tool success paths.

- [ ] **Step 5: Commit publication Elicitation**

```bash
git add crates/art-mcp/src/lib.rs crates/art-mcp/tests/elicitation_contracts.rs
git commit -m "feat: publish knowledge through separate elicitation"
```

### Task 4: Update skills, operator guidance, and 0.3.2 version surfaces

**Files:**
- Modify: `plugin/agent-recall-trail/skills/agent-recall-trail/SKILL.md`
- Modify: `integrations/dsh/art-recall/SKILL.md`
- Modify: `integrations/codex/README.md`
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/operations.md`
- Modify: `docs/testing.md`
- Modify: `CHANGELOG.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `plugin/agent-recall-trail/.codex-plugin/plugin.json`
- Modify: `docs/launch/launch-manifest.json`
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/publish-mcp.yml`
- Modify: `scripts/install.sh`
- Modify: `scripts/test_launch_surface.sh`
- Modify: `site/index.html`
- Modify: `tests/scripts/test_install_lifecycle.sh`
- Modify: `tests/scripts/test_plugin_surface.sh`
- Modify: `tests/scripts/test_release_version.sh`
- Test: `tests/scripts/test_release_version.sh`
- Test: `tests/scripts/test_plugin_surface.sh`

**Interfaces:**
- Consumes: the exact seven-tool runtime and stable fallback outputs.
- Produces: consistent 0.3.2 packages, plugin metadata, release surfaces, and Agent guidance.

- [ ] **Step 1: Write failing packaging assertions**

Change version assertions to `0.3.2`; change the plugin surface test to invoke
`tool_surface_is_exactly_seven_agent_safe_tools`; assert both skills contain
`art_knowledge_governance`, `Elicitation`, the ordinary-chat prohibition, and
CLI fallback wording.

Run:

```bash
ART_BIN=target/debug/art bash tests/scripts/test_release_version.sh
ART_BIN=target/debug/art bash tests/scripts/test_plugin_surface.sh
```

Expected: FAIL on the old version and six-tool guidance.

- [ ] **Step 2: Update version surfaces mechanically and review every match**

Set workspace/plugin/release candidate versions to `0.3.2`. Preserve historical
acceptance documents and old release plans as historical records; do not rewrite
their `0.3.0` or `0.3.1` claims. Update only active code, manifests, current
launch copy, workflows, installer checks, and tests.

- [ ] **Step 3: Update skills and documentation**

Document the sequence `propose -> governance(review) -> governance(publish)`,
that only Elicitation responses carry decisions, and that Agents must never run
operator review/publish commands. State that unsupported clients return a CLI
fallback requiring direct user action.

- [ ] **Step 4: Run version and plugin tests**

Run the two commands from Step 1 after building `target/debug/art`.

Expected: PASS, including restricted-PATH, exact seven-tool, skill/tool-name,
and version consistency checks.

- [ ] **Step 5: Commit documentation and versioning**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.md crates plugin integrations docs .github scripts site tests
git commit -m "docs: prepare ART 0.3.2 elicitation release"
```

### Task 5: Full isolated verification and local Codex acceptance

**Files:**
- Create: `docs/artifacts/art-0.3.2-elicitation-acceptance-2026-09-06.md`
- Modify only if a verified defect is found: files from Tasks 1-4

**Interfaces:**
- Consumes: the complete 0.3.2 candidate.
- Produces: reproducible acceptance evidence with an exact statement of native Codex Elicitation support or fallback behavior.

- [ ] **Step 1: Start task-scoped residue observation or record the tool boundary**

Use Agent Residue Evidence when its MCP tools are exposed. Otherwise record that
the formal observer is unavailable, capture baseline task processes and exact
temporary roots manually, and do not widen inspection beyond the repository,
task-owned target, and task-owned temporary homes.

- [ ] **Step 2: Run the Rust and release gates in isolated storage**

Create an exact task-owned Cargo target directory and isolated ART homes, then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
bash tests/scripts/release-gate.sh
```

Expected: all required gates pass with zero unexplained skips.

- [ ] **Step 3: Run protocol acceptance with a supporting client**

Use the rmcp paired-client harness and a packaged stdio process to prove review,
request-changes, reject, publish, decline, cancel, unsupported capability,
disconnect, and reconnect. Verify all fixtures use disposable ART homes and that
formal proposal/Edition counts do not change.

- [ ] **Step 4: Run a real isolated local Codex session**

Create a disposable Codex home containing only the candidate plugin and point it
to a disposable ART home and candidate binary. Start a fresh local Codex process
with restricted host PATH and ask it to discover and invoke
`art_knowledge_governance` against a fixture Proposal.

If Codex advertises form Elicitation, have the local user approve the disposable
fixture and separately confirm publication, then exact-read the resulting
Edition. If Codex does not advertise it, assert `ELICITATION_UNSUPPORTED`, verify
zero writes, and record that the rmcp harness—not Codex—proved the positive path.

- [ ] **Step 5: Build and verify the candidate package**

Build the release binary and package using repository scripts, verify archive
inventory, version, source identity, credentials/private paths, license
metadata, checksum, restricted-PATH launch, EOF shutdown, and reconnect. Do not
install or publish the candidate into the formal ART/Codex homes.

- [ ] **Step 6: Write the acceptance report and inspect residue**

Record exact commit, tool surface, test counts, Codex client capability result,
candidate checksums, formal-state before/after counts, skipped optional probes,
and residue boundaries. Remove only task-owned disposable homes, processes,
archives, logs, and target data whose ownership is proven; retain the source,
spec, plan, acceptance report, and candidate checksum evidence.

- [ ] **Step 7: Commit final acceptance evidence**

```bash
git add docs/artifacts/art-0.3.2-elicitation-acceptance-2026-09-06.md
git commit -m "test: accept ART 0.3.2 elicitation governance"
```

### Task 6: Final review and handoff

**Files:**
- Inspect: all branch changes since `v0.3.1`

**Interfaces:**
- Consumes: Tasks 1-5 and their commits.
- Produces: a clean, reviewable branch with no release, tag, push, formal installation, or Knowledge Edition publication.

- [ ] **Step 1: Review the complete diff and history**

Run:

```bash
git diff --check v0.3.1..HEAD
git diff --stat v0.3.1..HEAD
git log --oneline --decorate v0.3.1..HEAD
git status --short --branch
```

Expected: no whitespace errors, intentional files only, clean worktree.

- [ ] **Step 2: Re-run the shortest release-blocking smoke set**

Run the exact seven-tool MCP contract, Elicitation contracts, release version
contract, plugin surface contract, and `art doctor` against the disposable
candidate home.

- [ ] **Step 3: Report the boundary accurately**

State what passed, whether real Codex supported native Elicitation, what fallback
was observed, branch and commits, and what remains intentionally unperformed:
merge, public release, tag, push, formal installation, and publication of the
user's existing Knowledge Proposal.
