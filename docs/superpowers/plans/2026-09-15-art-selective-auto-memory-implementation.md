# ART Selective Automatic Memory Implementation Plan

> Execute with `superpowers:executing-plans`; every production behavior follows
> test-driven development and fresh completion verification.

**Goal:** Deliver an opt-in, machine-wide, selective Codex automatic-memory
candidate flow with conservative policy activation, governance controls,
packaging, and full candidate acceptance without a public release.

**Spec:** `docs/superpowers/specs/2026-09-15-art-selective-auto-memory-design.md`

## Constraints

- The machine-wide setting defaults and fails closed to disabled.
- Only the authenticated governance UI changes it; Agent/MCP surfaces read it.
- Hook, submit, and activation independently re-read the setting.
- No raw prompt/transcript persistence, session-end model, independent model,
  or automatic shared-knowledge publication.
- Automated fixtures use task-owned ART homes and never mutate formal state.
- Existing explicit capture, recall, and DSH contracts remain compatible.

## Task 1: Global setting and durable receipts

**Files:** add/modify `art-agent-store` domain/storage modules and contract tests.

- [x] Write failing tests for missing/malformed/unreadable/default-off,
  owner-only persistence, restart, multi-Agent visibility, concurrent updates,
  and independent delegated-governance configuration.
- [x] Add versioned atomic local configuration with safe file permissions.
- [x] Add trigger/submission/policy receipt schema and transactional APIs.
- [x] Prove migration backup/restore and unchanged explicit capture behavior.

## Task 2: Candidate submission and policy activation

**Files:** modify `art-domain`, `art-agent-store`, and their tests.

- [x] Write failing tests for candidate-first storage, identity binding,
  idempotent replay, key/content conflict, mid-flight disable, duplicates,
  conflicts, source expiry, sensitive input, file/Git verification, and scoped
  command/test evidence.
- [x] Implement structured auto-candidate and policy-decision types.
- [x] Implement conservative policy evaluation and single-transaction optional
  activation/index/receipt.
- [x] Keep uncertain or replacement candidates pending with reasons.

## Task 3: Read-only MCP status and candidate tool

**Files:** modify `art-mcp`, CLI/E2E contracts, MCP/package schemas.

- [x] Write failing protocol tests for `art_memory_candidate_submit`, forbidden
  authority fields, bound identity, disabled outcomes, read-only setting
  status, and existing tool compatibility.
- [x] Add the MCP tool and stable bounded responses.
- [x] Recheck the setting on submit and activation; expose diagnostics without
  private content.
- [x] Update generated schemas and tool-count contracts.

## Task 4: Codex hook classifier and rate controls

**Files:** add hook runtime under `art-cli`/plugin, hook configuration, tests,
and Codex integration docs.

- [x] Write at least sixty literal Chinese/English positive, negative,
  sensitive, quoted-memory, unresolved-failure, and malformed cases before
  implementing the classifier.
- [x] Write failing tests for default-off zero work, prompt correction signals
  without prompt retention, `stop_hook_active`, one continuation, three/session,
  ten-minute cooldown, duplicate turns, restart, and no disabled-period replay.
- [x] Implement a lightweight stdin/stdout hook command that fails open.
- [x] Register `UserPromptSubmit` and `Stop` in the plugin and document the
  normal Codex trust flow.
- [x] Run 100 non-trigger measurements; assert zero continuation/model-token
  requests and local p95 <=100 ms.

## Task 5: Governance settings and candidate review

**Files:** modify embedded governance HTML/CSS/JS, server routes, Vault review
APIs, and browser/HTTP tests.

- [x] Write failing tests for authenticated setting read/update, CSRF/Origin,
  expiry, audit, global semantics, pending candidate details, confirm,
  edit-confirm, reject, stale revision, and in-flight disable.
- [x] Add the accessible global switch, limits/support/status diagnostics, and
  clear enabled/hook/capture distinctions.
- [x] Add bound-Agent candidate review with safe source/policy metadata.
- [x] Verify Codex in-app browser interaction on final candidate bytes.

## Task 6: Documentation, package, and DSH compatibility

**Files:** update skills, README/operations/security/testing documents,
manifest/package checks, and DSH integration tests.

- [x] Update instructions so automatic candidate use is opt-in and explicit
  capture remains available while disabled.
- [x] Build a candidate plugin and verify archive contents, hashes, launch, and
  upgrade/uninstall/migration paths in isolated homes.
- [x] Run DSH recall/capture/governance compatibility and confirm no Codex hook
  dependency leaks into DSH.

## Task 7: Final verification and evidence

**Files:** create
`docs/artifacts/art-selective-auto-memory-acceptance-2026-09-15.md`.

- [x] Run format, Clippy, all workspace tests/features, generated artifact,
  open-source, release-version, plugin surface, install lifecycle, migration,
  backup/restore, and package verification gates.
- [x] Run isolated end-to-end flow: verified result -> continuation -> Candidate
  -> automatic activation or pending review -> exact recall/read.
- [x] Exercise default-off and runtime-disable boundaries through real Codex
  CLI/App, plus the governance page in the in-app browser.
- [x] Record sanitized receipts, package hashes, policy/config versions, test
  counts, performance distribution, skips, and residue limitations.
- [x] Stop task-owned processes and clean only task-created temporary state;
  keep source documents, final candidate package, and acceptance report.
- [x] Capture the bounded reusable implementation conclusion in ART.
