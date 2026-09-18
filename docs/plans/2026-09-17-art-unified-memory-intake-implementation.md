# ART Unified Private-Memory Intake Implementation Plan

**Spec:** `docs/specs/2026-09-17-art-unified-memory-intake-design.md`

## Global constraints

- Work only on the existing isolated `codex/art-auto-memory` branch.
- Use test-driven development and task-owned `ART_HOME` fixtures.
- Preserve physical per-Agent Vault isolation and shared-knowledge governance.
- Do not write automated fixtures to the formal local ART Vault.
- Do not tag, push, publish, or enter the public release process.

## Task 1: Unified intake model, migration, and policy service

Write failing store tests for origin validation/defaulting, explicit intake
while disabled, automatic intake while disabled, unified dispositions,
Agent/day fallback limits, cross-origin idempotency/duplicates, runtime disable,
and automatic revision proposals that preserve the Active revision. Prove the
failures against the current implementation.

Add the origin, attribution, intake request/result, receipt schema, incremental
Vault migration, and one transaction-level intake service. Move the current
automatic evidence/sensitivity/duplicate/conflict behavior behind that service.
Keep historical receipts as `legacy_unspecified`. Make all tests green and
verify migration backup/restore.

## Task 2: MCP adapters and Hook coordination

Write failing MCP and Hook tests for the new capture fields, omitted-origin
default, request/value reason requirements, unified output, proactive automatic
capture without Hook, exact Hook receipt handling, shared budget, linked-turn
Hook suppression, and untrusted/absent session identity fallback.

Adapt `art_memory_capture` and `art_memory_candidate_submit` to the unified
service. Update Stop/UserPromptSubmit coordination without transcript storage.
Keep Hook single-continuation and `stop_hook_active` protections. Make focused
MCP/CLI/Hook tests green and regenerate checked schemas through the repository's
normal generator.

## Task 3: Governance UI, instructions, and compatibility

Write failing route/UI behavior tests for origin diagnostics, combined switch
copy, shared budget state, pending revision details, and review actions.

Update the loopback UI and diagnostic endpoints. Update the ART skill, plugin
manifest/tool guidance, and Codex/DSH documentation so both automatic origins
use the same value standard and explicit user requests declare a sanitized
request basis. Verify DSH has proactive capture compatibility without adding a
DSH Hook.

## Task 4: Quality corpus, reliability, and package gates

Extend the bilingual corpus to at least sixty positive, negative, malformed,
duplicate, conflict, sensitive, proactive, and Hook cases. Test both guidance
paths and report program-policy results separately from real-Agent trials.

Exercise concurrency, retry, runtime disable, restart, migration restore, and
100-run non-trigger performance. Run format, Clippy, all workspace tests,
generated artifact checks, open-source/package checks, and security/advisory
checks required by the repository. Fix every introduced failure.

## Task 5: Candidate packaging and real local acceptance

Build a uniquely identified local candidate archive and record its SHA-256,
source revision, config schema, policy version, and test evidence. Verify clean
install, same-version repair, upgrade from the installed 0.3.5 plugin, rollback,
and uninstall in isolated homes.

Preserve the formal automatic-memory switch value, install the final candidate
into local Codex, restart/reconnect the consuming runtime, and verify installed
bytes and MCP health. Through real Codex CLI/App, prove explicit user-requested
capture, proactive Agent capture without Hook dependence, Hook no-op and Hook
candidate paths, duplicate suppression, switch-off boundaries, exact recall,
and in-app governance review. Run DSH explicit/proactive regression against the
installed payload without enabling a DSH Hook.

Create `docs/artifacts/art-unified-memory-intake-acceptance-2026-09-17.md`
with sanitized receipts and limitations. Remove only task-owned temporary test
state after evidence is recorded; retain the candidate package, source, and
acceptance report. Do not publish.
