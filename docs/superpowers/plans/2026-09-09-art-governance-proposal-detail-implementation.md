# ART 0.3.5 Knowledge Proposal Detail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the complete ART 0.3.5 page-based knowledge-proposal detail, human review, rejection, and publication workflow with exact-revision security, safe Markdown, accessible responsive UI, final packaging, and Codex/DSH acceptance.

**Architecture:** Extend `KnowledgeVault` with bounded read models for review history and hash-verified current Edition content. Extend the embedded loopback governance server with a summary-only bootstrap, an exact authorized detail endpoint, strict mutation snapshots, security headers, Markdown sanitization, and grouped line diffs. Replace queue mutations with a modal right-drawer state machine whose only queue action is “Review details,” while preserving existing settings/audit behavior and delegated-governance semantics.

**Tech Stack:** Rust 1.98, Axum 0.8, SQLite/rusqlite, pulldown-cmark 0.13.4, ammonia 4.1.4, similar 3.2.0, embedded HTML/CSS/vanilla JavaScript, reqwest contract tests, browser automation, Bash/Python release gates.

**Spec:** `docs/superpowers/specs/2026-09-09-art-governance-proposal-detail-design.md`

## Global Constraints

- Bind governance only to a random `127.0.0.1` port and keep the capability plus CSRF/Origin controls.
- Fix every human governance session lifetime at 30 minutes; warn when five minutes remain.
- Keep `GET /api/bootstrap` summary-only; never expose proposal Markdown or private source bodies there.
- Exact-target sessions may read and mutate only their bound Proposal ID and revision.
- Mutation boundaries compare revision, status, draft hash, and source-set hash and fail without a write on drift.
- Render with `pulldown-cmark` 0.13.4, sanitize with `ammonia` 4.1.4, and diff with `similar` 3.2.0.
- Do not add MCP tools, database migrations, remote listeners, editable proposal content, CLI-first governance, or changes to delegated-governance semantics.
- All automated and browser fixtures use a task-owned `ART_HOME`; formal `~/.across` knowledge and governance history remain untouched.
- Use TDD for every behavior: name the production break, observe the focused test fail for the expected reason, add the minimal implementation, and rerun the focused suite.

---

### Task 1: Knowledge review read model and verified Edition Markdown

**Files:**
- Modify: `crates/art-knowledge/src/lib.rs`
- Modify: `crates/art-knowledge/tests/knowledge_contracts.rs`

**Interfaces:**
- Produces: `ProposalReviewRecord { id, proposal_id, proposal_revision, source_set_hash, decision, actor, reason, decided_at }`.
- Produces: `VerifiedEdition { record: EditionRecord, canonical_markdown: String }`.
- Produces: `KnowledgeVault::proposal_reviews(&self, id: &str, revision: u32) -> ArtResult<Vec<ProposalReviewRecord>>` ordered oldest-first.
- Produces: `KnowledgeVault::verified_current(&self, knowledge_key: &str) -> ArtResult<Option<VerifiedEdition>>`, which re-hashes the Edition Markdown and manifest before returning content.
- Consumes: existing `KnowledgeVault::proposal`, `KnowledgeVault::current`, projection records, and stored manifest hashes.

- [ ] **Step 1: Write failing domain tests** for ordered review reasons, exact revision filtering, absent current Edition, valid current Edition content, and tampered Markdown/manifest rejection. Each expected record and hash is derived from literal fixture content.
- [ ] **Step 2: Run the focused tests and observe RED.** Run `cargo test -p art-knowledge --test knowledge_contracts proposal_review_history -- --exact --nocapture` and the verified-current tests; expect missing methods/types.
- [ ] **Step 3: Implement the bounded read models.** Query only the requested proposal revision, parse RFC3339 times, canonicalize paths beneath the Edition root, read bounded UTF-8 Markdown/manifest files, and compare stored SHA-256 values before returning `VerifiedEdition`.
- [ ] **Step 4: Run focused and crate tests GREEN.** Run `cargo test -p art-knowledge --test knowledge_contracts` and `cargo clippy -p art-knowledge --all-targets -- -D warnings`.

### Task 2: Exact proposal-detail contract, rendering, and comparison

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/art-mcp/Cargo.toml`
- Modify: `crates/art-mcp/src/governance_ui.rs`
- Modify: `crates/art-mcp/tests/governance_ui_contracts.rs`

**Interfaces:**
- Produces: `GET /api/proposal-detail?session=<capability>&proposal_id=<id>&revision=<n>` with schema `art.governance.proposal-detail.v1`.
- Produces: a JSON detail model containing exact proposal metadata; canonical combined Applicability/Knowledge Markdown; sanitized HTML; draft/source-set hashes; locked source metadata; ordered reviews; optional verified current Edition; grouped three-line-context diff; predicted next Edition number; allowed actions; and outstanding review requirements.
- Produces: common response middleware/helper applying `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, and `X-Content-Type-Options: nosniff` to HTML, assets, and APIs.
- Consumes: Task 1 `proposal_reviews` and `verified_current`.

- [ ] **Step 1: Write failing HTTP contract tests** proving the bootstrap actionable count/filter and absence of full Markdown/private source bodies, exact-target auto-open metadata, general-session allowed reads, cross-proposal denial, first-Edition comparison, replacement diff, safe rendered Markdown, and the three security headers.
- [ ] **Step 2: Write malicious Markdown fixtures** containing scripts, `javascript:` links, event handlers, inline styles, iframes, raw active HTML, and remote images; assert the detail HTML contains none of those active forms and does not embed the session capability.
- [ ] **Step 3: Run focused tests RED.** Run `cargo test -p art-mcp --test governance_ui_contracts proposal_detail -- --nocapture`; expect 404/missing schema and missing dependencies.
- [ ] **Step 4: Add the exact dependencies and endpoint.** Build canonical review Markdown as `## Applicability\n\n{applicability}\n\n## Knowledge\n\n{markdown}`, render tables/strikethrough/task lists with `pulldown-cmark`, sanitize using an explicit allowlist with remote images removed and safe link protocols only, and build `similar::TextDiff` groups with three context lines.
- [ ] **Step 5: Enforce read authorization before vault lookup.** Exact sessions require both bound fields to equal the query; general pending sessions may read only IDs/revisions in their actionable authorized queue snapshot.
- [ ] **Step 6: Run focused and crate tests GREEN.** Run `cargo test -p art-mcp --test governance_ui_contracts` and `cargo clippy -p art-mcp --all-targets -- -D warnings`.

### Task 3: Exact review/rejection/publication receipts and conflict safety

**Files:**
- Modify: `crates/art-knowledge/src/lib.rs`
- Modify: `crates/art-knowledge/tests/knowledge_contracts.rs`
- Modify: `crates/art-mcp/src/governance_ui.rs`
- Modify: `crates/art-mcp/tests/governance_ui_contracts.rs`

**Interfaces:**
- Produces: review requests containing `session`, `proposal_id`, `revision`, `status`, `draft_hash`, `source_set_hash`, `decision`, and trimmed `reason` of 1–1000 characters.
- Produces: review response `{ schema, ok, proposal_id, revision, decision, status, review }` from the authoritative committed proposal/review record.
- Produces: publish requests containing the same exact snapshot plus `predicted_edition_number` and `confirm: true`.
- Produces: publication receipt with Edition ID/number, publication time, Markdown SHA-256, and manifest SHA-256.

- [ ] **Step 1: Write failing tests** for blank/oversized reasons, `approved`, `changes_requested`, and `rejected` authoritative status/receipt responses, rejection terminality, stale status/draft/source hashes, wrong predicted Edition number, expired session, wrong Origin/CSRF, and no-write assertions after every failure.
- [ ] **Step 2: Run focused tests RED.** Run the named mutation tests in `art-knowledge` and `art-mcp`; expect missing snapshot fields/receipts or overly permissive behavior.
- [ ] **Step 3: Implement minimal exact mutations.** Validate the client snapshot against the current proposal before calling `review_exact`/`publish_exact`, bound reason length after trimming, return committed records, and map expiry/authorization/conflict/input errors to stable status codes and response schemas.
- [ ] **Step 4: Preserve independent-review rules.** Keep elevated/high-risk distinct-reviewer enforcement and expose its outstanding requirement without inventing a new identity mechanism.
- [ ] **Step 5: Run focused and crate tests GREEN.** Run `cargo test -p art-knowledge --test knowledge_contracts`, `cargo test -p art-mcp --test governance_ui_contracts`, and the existing Elicitation contracts to prove delegated governance did not change.

### Task 4: Accessible responsive proposal review drawer

**Files:**
- Modify: `crates/art-mcp/assets/governance/index.html`
- Modify: `crates/art-mcp/assets/governance/app.js`
- Modify: `crates/art-mcp/assets/governance/styles.css`
- Modify: `crates/art-mcp/tests/governance_ui_contracts.rs`
- Create if repository browser harness supports it: `tests/browser/governance-proposal-detail.mjs`

**Interfaces:**
- Queue produces exactly one primary `Review details` action per actionable proposal and displays the real actionable count.
- Native `<dialog>` detail surface provides Knowledge, Raw Markdown, and Changes tabs; source locks; current Edition; review history; requirements; decision form; publication confirmation; receipt; warning/conflict/expired states.
- JavaScript functions `openProposal(proposal, trigger)`, `loadProposalDetail(id, revision)`, `renderDetail(detail)`, `openDecisionForm(decision)`, `submitReview()`, `openPublishConfirmation()`, `submitPublish()`, `closeDetail()`, `enterConflict(message)`, and `enterExpired(message)` maintain one explicit state machine.

- [ ] **Step 1: Write failing asset/browser assertions** for no queue shortcut decisions, dialog semantics/name/description, tab semantics, labeled required reason field, persistent live region, 30-minute timer with five-minute warning, exact-target auto-open, conflict/expiry control disabling, and publish receipt fields.
- [ ] **Step 2: Run asset/browser tests RED** against the embedded server and task-owned proposal fixtures; expect missing dialog/detail controls.
- [ ] **Step 3: Implement semantic HTML and client state.** Use DOM `textContent` for raw/source/review data and inject only the server-sanitized HTML into the rendered-content container; never interpolate capability or private source bodies into markup.
- [ ] **Step 4: Implement focus and keyboard behavior.** On open, remember the queue trigger and focus the close button; trap Tab/Shift+Tab; implement Arrow/Home/End tab movement; close on Escape only when no mutation or unsaved confirmation is active; restore focus to the originating card; announce load/decision/conflict/expiry/publication.
- [ ] **Step 5: Implement the approved ART visual direction.** Desktop dialog is a right drawer `width: min(880px, 72vw)` with sticky header/footer, ~72-character reading measure, and provenance spine `Draft → Sources → Decision → Publication`; under 760 px it fills the viewport. Status/diff meanings use text plus semantic markup, focus rings remain visible, and reduced motion removes translation.
- [ ] **Step 6: Run desktop, 759 px, keyboard, reduced-motion, malicious-content, review, reject, changes, publish, expiry, reconnect, and conflict browser journeys GREEN.** Capture task IDs, viewport, result, and screenshots/log receipts in a task-owned evidence directory.

### Task 5: 0.3.5 version, documentation, package, and release consistency

**Files:**
- Modify every authoritative `0.3.4` release surface resolved by `rg`, including `Cargo.toml`, `Cargo.lock`, `README.md`, `CHANGELOG.md`, `site/index.html`, installer/launch scripts, packaging manifests, workflows, and version contract fixtures.
- Create: `docs/artifacts/art-0.3.5-governance-proposal-detail-acceptance-2026-09-09.md`

**Interfaces:**
- Produces: version-consistent `art 0.3.5` binaries, native archives, `.mcpb`, registry metadata, plugin metadata, site/release metadata, and a dedicated acceptance record.
- Preserves: historical 0.3.4 acceptance documents and dependency versions such as `getrandom 0.3.4`; only current release surfaces change.

- [ ] **Step 1: Write/update failing version and package tests** so the expected current version is 0.3.5 and candidate assets must contain the new governance detail route/assets/dependencies.
- [ ] **Step 2: Run version/package tests RED.** Run `bash tests/scripts/test_release_version.sh` and `bash scripts/test_launch_surface.sh`; expect current 0.3.4 metadata failures.
- [ ] **Step 3: Update authoritative release surfaces** to 0.3.5 while leaving historical evidence and unrelated dependency versions unchanged.
- [ ] **Step 4: Regenerate lock/license/SBOM artifacts through repository scripts** and inspect diffs for only expected dependency/version additions.
- [ ] **Step 5: Run version, launch, dependency, license, and package checks GREEN.** Build final release assets from the candidate commit/tree and verify embedded `index.html`, `app.js`, `styles.css`, and proposal-detail route behavior from the packaged binary.

### Task 6: Full regression, installed-host acceptance, and residue cleanup

**Files:**
- Complete: `docs/artifacts/art-0.3.5-governance-proposal-detail-acceptance-2026-09-09.md`
- Do not modify: formal user ART knowledge/proposal/governance state.

**Interfaces:**
- Consumes: final built 0.3.5 package and task-owned ART homes/proposals.
- Produces: source → packaged payload → installed runtime → live MCP → visible page evidence for Codex Desktop and DSH.

- [ ] **Step 1: Run format/static/unit/integration gates.** Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `bash tests/scripts/release-gate.sh`; record counts, the one intentional performance ignore if still present, and any environment-only skips.
- [ ] **Step 2: Build and verify the final 0.3.5 candidate.** Run repository release build and verification scripts against a task-owned output directory, check versions/checksums/signatures where available, and prove the embedded governance assets are the final source bytes.
- [ ] **Step 3: Install the candidate into isolated Codex and DSH test environments** using task-owned `ART_HOME` and temporary opt-in host configuration; do not replace or mutate the user’s formal ART configuration.
- [ ] **Step 4: Complete visible Codex and DSH journeys.** In each host, open an exact proposal through the real `art_governance_ui_open` path and visibly exercise detail inspection, first/replacement diff, source locks/history, approve, request changes, reject, and two-step publish with keyboard-only desktop and narrow layouts.
- [ ] **Step 5: Verify security failure journeys.** Exercise malicious Markdown, wrong proposal/revision, stale snapshot, expired session, lost server, bad Origin/CSRF, and independent-review limitation; confirm disabled/no-write behavior and copy-preserved entered reason.
- [ ] **Step 6: Clean task residue.** Stop attributed listeners/processes, remove only task-created ART homes, proposals, temporary host configurations, browser profiles, release staging, and build intermediates whose purpose ended; retain the final deliverables and acceptance artifact.
- [ ] **Step 7: Re-run final Git and residue checks.** Report branch/worktree state, exact retained files, final disk impact, attributed listening ports/processes, required/optional skipped coverage, and whether any user-owned untracked files remain outside the worktree.
