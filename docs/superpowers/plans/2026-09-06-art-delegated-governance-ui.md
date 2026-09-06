# ART Delegated Governance UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Ship ART 0.3.3 with one page-first governance experience for Codex Desktop and DSH plus persistent, distinctly audited, single-instruction delegated approval and publication.

**Architecture:** The Knowledge Vault owns delegation policy, delegated actor receipts, and one exact atomic approve-and-publish operation. ART MCP exposes the operation and starts an embedded loopback web application whose session-bound endpoints use the same domain service. Codex and DSH skills route people to the page and never to CLI.

**Tech Stack:** Rust 1.98, rusqlite, rmcp 3.1.4, Tokio, Axum, embedded HTML/CSS/JavaScript, shell/Python packaging checks, Codex Desktop in-app browser, DSH page surface.

**Spec:** docs/superpowers/specs/2026-09-06-art-delegated-governance-ui-design.md

## Global Constraints

- Human-facing settings, review, publication, status, and audit are page-only.
- Delegated mode is missing-row default-off and persists by host binding plus Agent ID.
- Delegated mode covers every ART risk level and accepts one unambiguous user instruction.
- Delegated execution is always labeled AgentDelegated and never Human.
- Approval and publication are one logical atomic operation with no visible approved-only state.
- The Agent never supplies actor, reason, risk override, content hashes, or confirmation.
- The page binds only to a random 127.0.0.1 port and uses bounded expiring session capabilities.
- Tests use task-owned ART_HOME directories and never write formal local ART data.
- Existing memory, recall, Elicitation, backup, and recovery behavior stays compatible.
- Versioned packages and plugin artifacts move together to 0.3.3.

---

### Task 1: Delegated actor and repository contract

**Files:**
- Modify: crates/art-domain/src/knowledge.rs
- Modify: crates/art-domain/tests/domain_contracts.rs
- Modify: AGENTS.md

**Interfaces:**
- Produces: ReviewActor::AgentDelegated { agent_id: AgentId, host_binding_hash: String, authorization_basis: DelegatedAuthorizationBasis }
- Produces: DelegatedAuthorizationBasis::CurrentUserInstruction

- [ ] **Step 1: Write failing domain tests**

Add tests proving Human, Agent, and AgentDelegated serialize to distinct tagged
representations; reject empty host hashes; and prove AgentDelegated never
matches the Human variant.

    let actor = ReviewActor::AgentDelegated {
        agent_id: AgentId::parse("codex-primary").unwrap(),
        host_binding_hash: "a".repeat(64),
        authorization_basis:
            DelegatedAuthorizationBasis::CurrentUserInstruction,
    };
    let encoded = serde_json::to_value(&actor).unwrap();
    assert_eq!(encoded["type"], "agent_delegated");
    assert_ne!(encoded["type"], "human");

- [ ] **Step 2: Run the focused test and observe failure**

Run: cargo test -p art-domain --test domain_contracts delegated_actor --locked

Expected: compilation fails because AgentDelegated and
DelegatedAuthorizationBasis do not exist.

- [ ] **Step 3: Implement the minimal actor types**

Use an explicitly tagged serde representation. Validate the 64-character
lowercase hexadecimal host-binding hash at construction or domain admission.
Do not overload the existing Agent variant.

- [ ] **Step 4: Update the repository rule**

Replace “Agents never approve or publish knowledge” with: Agents cannot perform
human governance. A bound Agent may perform distinctly labeled delegated
governance only while persistent local policy is enabled.

- [ ] **Step 5: Run focused domain tests**

Run: cargo test -p art-domain --test domain_contracts --locked

Expected: all domain contracts pass.

- [ ] **Step 6: Commit**

Commit: feat: add delegated governance actor

---

### Task 2: Persistent delegation policy

**Files:**
- Modify: crates/art-knowledge/src/lib.rs
- Modify: crates/art-knowledge/tests/knowledge_contracts.rs

**Interfaces:**
- Consumes: AgentId
- Produces: DelegationMode::{HumanReview, DelegatedLocal}
- Produces: KnowledgeVault::delegation_mode(agent_id, host_binding_hash)
- Produces: KnowledgeVault::set_delegation_mode(agent_id, host_binding_hash, mode, changed_via)

- [ ] **Step 1: Write failing storage contracts**

Cover missing-row default-off, enable persistence after reopen, isolation by
Agent and host, immediate disable, invalid hash rejection, and append-only
configuration audit.

    assert_eq!(
        vault.delegation_mode(&agent, &host_hash).unwrap(),
        DelegationMode::HumanReview
    );
    vault.set_delegation_mode(
        &agent,
        &host_hash,
        DelegationMode::DelegatedLocal,
        "local_governance_ui",
    ).unwrap();

- [ ] **Step 2: Observe the focused failure**

Run: cargo test -p art-knowledge --test knowledge_contracts delegation_policy --locked

Expected: compilation fails because the policy API does not exist.

- [ ] **Step 3: Add schema and policy API**

Add delegation_policies keyed by host_binding_hash plus agent_id and
delegation_policy_events as append-only audit. Missing rows return HumanReview.
Use one transaction for policy row and event.

- [ ] **Step 4: Verify storage contracts**

Run: cargo test -p art-knowledge --test knowledge_contracts delegation_policy --locked

Expected: policy contracts pass.

- [ ] **Step 5: Commit**

Commit: feat: persist delegated governance policy

---

### Task 3: Atomic delegated approve-and-publish

**Files:**
- Modify: crates/art-knowledge/src/lib.rs
- Modify: crates/art-knowledge/tests/knowledge_contracts.rs

**Interfaces:**
- Consumes: GovernanceSnapshot, ReviewActor::AgentDelegated, DelegationMode
- Produces: KnowledgeVault::approve_and_publish_delegated_exact(snapshot, actor, host_binding_hash) -> KnowledgeEdition

- [ ] **Step 1: Write failing atomicity tests**

Cover all risk levels, disabled policy, stale source, stale snapshot,
concurrent disable, concurrent duplicate calls, competing Edition allocation,
and injected failures before and after projection. Assert no Approved state is
observable and exactly one Edition is current.

    let edition = vault
        .approve_and_publish_delegated_exact(&snapshot, actor, &host_hash)
        .unwrap();
    assert_eq!(
        vault.proposal(&snapshot.proposal_id).unwrap().status,
        ProposalStatus::Materialized
    );
    assert_eq!(vault.delegated_receipts(&snapshot.proposal_id).unwrap().len(), 2);

- [ ] **Step 2: Observe failure**

Run: cargo test -p art-knowledge --test knowledge_contracts delegated_publish --locked

Expected: compilation fails because the atomic API does not exist.

- [ ] **Step 3: Implement transaction and recovery integration**

Reuse canonical source/snapshot checks and the existing reservation,
publish-intent, manifest, projection, and recovery machinery. Record linked
delegated review and publication events. Recheck policy within the mutation
boundary. Never call review_exact followed by publish_exact as two externally
visible operations.

- [ ] **Step 4: Verify knowledge contracts**

Run: cargo test -p art-knowledge --test knowledge_contracts --locked

Expected: all knowledge contracts pass, including old human-only tests adjusted
to distinguish unconfigured Agents from configured AgentDelegated actors.

- [ ] **Step 5: Commit**

Commit: feat: atomically publish delegated knowledge

---

### Task 4: MCP delegated operation and bounded status

**Files:**
- Modify: crates/art-mcp/src/lib.rs
- Modify: crates/art-mcp/tests/elicitation_contracts.rs
- Modify: crates/art-mcp/tests/mcp_contracts.rs

**Interfaces:**
- Consumes: KnowledgeVault::approve_and_publish_delegated_exact
- Produces: KnowledgeGovernanceOperation::ApproveAndPublish
- Produces: art_health.governance_mode

- [ ] **Step 1: Write failing MCP contracts**

Assert the enum accepts approve_and_publish, input still has exactly operation,
proposal_id, and revision, disabled policy returns DELEGATION_DISABLED with
zero writes, enabled policy returns materialized Edition metadata, every risk
level works, and existing review/publish still elicit separately.

    let result = harness.call(
        "art_knowledge_governance",
        json!({
            "operation": "approve_and_publish",
            "proposal_id": proposal.id,
            "revision": proposal.revision
        }),
    ).await;
    assert_eq!(result["outcome"], "published");
    assert_eq!(result["actor_type"], "agent_delegated");

- [ ] **Step 2: Observe failure**

Run: cargo test -p art-mcp --test elicitation_contracts delegated --locked

Expected: schema or enum validation fails.

- [ ] **Step 3: Implement the MCP branch**

Derive AgentDelegated from the process-bound Agent ID and canonical host-binding
hash. Do not add tool inputs. Return proposal status, Edition reference,
actor_type, and stable reason codes. Add governance_mode to bounded health so
skills can route without opening a page.

- [ ] **Step 4: Verify MCP contracts**

Run: cargo test -p art-mcp --test elicitation_contracts --locked
Run: cargo test -p art-mcp --test mcp_contracts --locked

Expected: all new and existing contracts pass.

- [ ] **Step 5: Commit**

Commit: feat: expose delegated governance over MCP

---

### Task 5: Embedded loopback governance application

**Files:**
- Modify: Cargo.toml
- Modify: crates/art-mcp/Cargo.toml
- Create: crates/art-mcp/src/governance_ui.rs
- Create: crates/art-mcp/assets/governance/index.html
- Create: crates/art-mcp/assets/governance/styles.css
- Create: crates/art-mcp/assets/governance/app.js
- Create: crates/art-mcp/tests/governance_ui_contracts.rs
- Modify: crates/art-mcp/src/lib.rs

**Interfaces:**
- Produces: GovernanceUiManager::open_session(view, proposal_ref) -> GovernanceUiSession
- Produces: GovernanceUiSession { url, expires_at, view, proposal_id, revision }
- Produces: art_governance_ui_open
- Consumes: KnowledgeVault policy, proposal, review, publish, and audit APIs

- [ ] **Step 1: Add failing loopback and session tests**

Verify 127.0.0.1-only random binding, high-entropy opaque session, expiry,
same-origin and CSRF rejection, proposal revision binding, bounded JSON,
redaction, refresh safety, and shutdown with the MCP process.

    let session = manager.open_session(UiView::Settings, None).await.unwrap();
    assert_eq!(session.url.host_str(), Some("127.0.0.1"));
    assert_ne!(session.url.port(), Some(80));
    assert!(expired_request(&session).await.status().is_client_error());

- [ ] **Step 2: Observe failure**

Run: cargo test -p art-mcp --test governance_ui_contracts --locked

Expected: target and module are absent.

- [ ] **Step 3: Add the smallest server dependencies**

Use Axum and Tokio network features. Do not add Node, a template runtime,
external CDN, analytics, or remote assets.

- [ ] **Step 4: Implement session manager and API**

Create focused handlers for bootstrap, pending proposal read, audit read,
delegation read/update, human review, and human publish. Enforce capability,
origin, CSRF, expiry, Agent/host binding, and exact proposal revision on every
mutation. Share the Knowledge Vault service rather than opening another
uncoordinated writer.

- [ ] **Step 5: Implement embedded UI**

Reproduce the approved local-ledger prototype as responsive static assets.
Use semantic controls, keyboard focus, status text, reduced motion, and no
external resources. The Settings toggle persists through the real API. Pending
and Audit render real bounded data. Do not expose CLI copy.

- [ ] **Step 6: Add the MCP open tool**

Add art_governance_ui_open with strict view, optional proposal_id, and optional
revision fields. Return safe URL and expiry metadata. Update exact tool-count
contracts.

- [ ] **Step 7: Verify UI and MCP tests**

Run: cargo test -p art-mcp --test governance_ui_contracts --locked
Run: cargo test -p art-mcp --test mcp_contracts --locked

Expected: all pass.

- [ ] **Step 8: Commit**

Commit: feat: add local governance web application

---

### Task 6: Page-first Codex and DSH behavior

**Files:**
- Modify: plugin/agent-recall-trail/skills/agent-recall-trail/SKILL.md
- Modify: plugin/agent-recall-trail/skills/agent-recall-trail/agents/openai.yaml
- Modify: integrations/codex/art-recall/SKILL.md
- Modify: integrations/codex/README.md
- Modify: integrations/dsh/art-recall/SKILL.md
- Modify: integrations/dsh/README.md
- Modify: tests/scripts/test_plugin_surface.sh
- Modify: tests/scripts/test_plugin_launch.py

**Interfaces:**
- Consumes: art_health.governance_mode, art_governance_ui_open, approve_and_publish
- Produces: identical page-first routing semantics in Codex and DSH skills

- [ ] **Step 1: Write failing plugin-surface assertions**

Require both skills to describe default-off delegated mode, one-instruction
approve-and-publish, AgentDelegated labeling, page opening, ambiguity stop, and
no user-facing CLI fallback. Require the new tool in the exact surface.

- [ ] **Step 2: Observe failure**

Run: bash tests/scripts/test_plugin_surface.sh

Expected: missing page and delegated-governance statements.

- [ ] **Step 3: Update skills and host guides**

Route unambiguous user requests through proposal plus delegated operation only
when health says delegated_local. Otherwise open the ART page. In Codex use the
in-app browser capability; in DSH use its page/browser surface. Never instruct
the user to run a command. Keep Elicitation compatible for other hosts.

- [ ] **Step 4: Verify plugin launch and surface**

Run: bash tests/scripts/test_plugin_surface.sh
Run: python3 tests/scripts/test_plugin_launch.py

Expected: both pass.

- [ ] **Step 5: Commit**

Commit: feat: route governance through pages

---

### Task 7: Version, packaging, docs, and release contracts

**Files:**
- Modify: Cargo.toml
- Modify: Cargo.lock
- Modify: README.md
- Modify: CHANGELOG.md
- Modify: docs/operations.md
- Modify: docs/security-model.md
- Modify: docs/architecture.md
- Modify: docs/launch/launch-manifest.json
- Modify: docs/launch/launch-article.md
- Modify: plugin/agent-recall-trail/.codex-plugin/plugin.json
- Modify: packaging/mcp-registry/server.json.in
- Modify: packaging/mcpb/manifest.json.in
- Modify: scripts/install.sh
- Modify: scripts/test_launch_surface.sh
- Modify: tests/scripts/test_release_version.sh
- Modify: tests/scripts/test_install_lifecycle.sh
- Create: docs/artifacts/art-0.3.3-delegated-governance-acceptance-2026-09-06.md

**Interfaces:**
- Produces: one consistent 0.3.3 source/package/install contract

- [ ] **Step 1: Change release tests to expect 0.3.3**

Update exact version assertions and tool count. Run the focused version test
before changing production metadata.

Run: bash tests/scripts/test_release_version.sh

Expected: fail because source still reports 0.3.2.

- [ ] **Step 2: Update all versioned surfaces**

Move workspace packages, manifests, installer checks, launch metadata, registry
templates, changelog, and docs to 0.3.3. Explain page-first interaction and the
weaker delegated trust boundary. Remove CLI fallback from normal user guidance.

- [ ] **Step 3: Run packaging checks**

Run: bash tests/scripts/test_release_version.sh
Run: bash scripts/test_launch_surface.sh
Run: bash tests/scripts/test_install_lifecycle.sh

Expected: all pass in task-owned temporary homes.

- [ ] **Step 4: Commit**

Commit: docs: prepare ART 0.3.3 delegated governance

---

### Task 8: Full verification and installed-host acceptance

**Files:**
- Modify: docs/artifacts/art-0.3.3-delegated-governance-acceptance-2026-09-06.md

**Interfaces:**
- Consumes: exact candidate commit and package
- Produces: source-to-page-to-durable-state acceptance evidence

- [ ] **Step 1: Run source gates**

Run: cargo fmt --all -- --check
Run: cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
Run: cargo test --workspace --all-targets --all-features --locked
Run: bash tests/scripts/release-gate.sh

Expected: all required gates pass; only the documented large performance test
may be ignored in the ordinary workspace run and must pass in release-gate.

- [ ] **Step 2: Build and install an isolated candidate**

Use task-owned ART_HOME and host configuration roots. Never replace the formal
installation for automated tests. Verify binary version, exact tool discovery,
loopback page, enable/restart persistence, delegated publication, audit, disable
enforcement, human page flow, and no CLI guidance.

- [ ] **Step 3: Test Codex Desktop**

Install the candidate plugin in an isolated or reversible Codex test location,
open the real ART page in the in-app browser, enable delegation, start a fresh
task, issue one unambiguous memory-to-knowledge request, and verify the Edition
and AgentDelegated audit. Do not substitute terminal rendering for this check.

- [ ] **Step 4: Test DSH**

Launch DSH with a task-owned ART_HOME and the candidate overlay, open the same
page, repeat enable/restart/delegated publication/disable checks, and verify no
duplicated policy implementation.

- [ ] **Step 5: Record exact acceptance evidence**

Write tested commit, versions, commands, counts, host states, proposal/Edition
fixture references, browser evidence boundaries, skipped probes, and residue.
Do not include secrets, capabilities, raw transcripts, or formal user data.

- [ ] **Step 6: Final state verification**

Run: git diff --check
Run: git status --short
Run: git log --oneline --decorate -10

Expected: only intentional committed changes; no untracked test state.

- [ ] **Step 7: Commit acceptance**

Commit: test: accept ART 0.3.3 delegated governance
