# ART Selective Automatic Memory Design

**Status:** Approved for implementation

**Target:** post-0.3.5 small-scale candidate; no public release

## Purpose

ART will selectively propose private memories when a Codex task produces a
durable decision, a user correction, or a verified repair. Automatic work is
opt-in, bounded, attributable, and candidate-first. Explicit memory capture,
recall, and shared-knowledge governance keep their existing contracts.

## Global safety boundary

One machine-wide ART setting controls automatic memory for every Agent. It is
persisted in ART's local configuration layer and defaults to disabled when the
file is missing, malformed, unreadable, or unsupported. Only an authenticated
human governance session may change it; MCP tools expose read-only status.

Every automatic stage checks the setting independently:

1. the prompt-signal hook before recording a non-content correction marker;
2. the Stop hook before requesting one bounded continuation;
3. candidate submission before any durable write;
4. policy activation immediately before changing a Candidate to Active.

Turning the setting off never removes existing memories and does not affect
explicit `art_memory_capture`, recall, feedback, or knowledge review. Work
already executing may finish computation, but cannot submit or activate after
the switch is off. Re-enabling applies only to later turns; no disabled-period
prompt or transcript is retained for replay.

The setting is separate from delegated shared-knowledge governance.

## Trigger and cost model

Codex plugin lifecycle hooks provide the host integration. `UserPromptSubmit`
recognizes only correction intent and persists a bounded signal code plus
session/turn identity; it never stores the prompt. `Stop` consumes the latest
assistant message and host metadata, but never reads the transcript.

A deterministic bilingual classifier admits only:

- explicit decisions with an enduring rationale or operating consequence;
- user corrections that are reflected in the completed result;
- repairs accompanied by a bounded verification result.

It rejects greetings, ordinary Q&A, progress reports, speculative ideas,
unresolved failures, quoted/recalled memory, sensitive-looking content, and
already handled hook events. Non-triggering turns cause no model continuation
and no additional model tokens.

An admitted turn may request at most one continuation from the current Agent.
The continuation asks for the structured candidate or an explicit no-op. It
must not include or request the transcript. `stop_hook_active=true` is always a
no-op. Defaults are three admitted extractions per Agent/session and a ten
minute interval. Trigger receipts make Agent/session/turn handling idempotent.

## Candidate contract and policy

`art_memory_candidate_submit` binds ownership to the MCP process identity. Its
caller supplies a trigger receipt, candidate ordinal, structured memory
payload, scope, sensitivity, and bounded source anchors. It cannot supply an
owner, status, assurance result, reviewer, or policy actor.

Submission first creates a Candidate. A deterministic policy then checks:

- allowed and sufficiently narrow scope;
- evidence timestamp and supported source kind;
- secret, credential, raw-conversation, and unsafe-content patterns;
- duplicate content and idempotency consistency;
- semantic conflicts or attempted replacement of an existing conclusion;
- actual file digest or Git object identity for file/Git claims;
- verification scope for test and command receipts.

Only a safe, source-verifiable, non-conflicting candidate is automatically
activated. Ambiguous evidence, a possible conflict, or a replacement remains
Candidate for human review. Rejected sensitive input is not stored as memory.
Automatic activation records policy version, evidence, reason, and a system
policy actor. It is not human assurance.

## Persistence and recovery

Each Agent keeps its own Vault. The Vault schema gains automatic-candidate
receipt and policy-decision records. Submission identity is unique by trigger
receipt and candidate ordinal. Same-key/same-content retries return the prior
outcome; same-key/different-content retries fail as conflicts.

Candidate insertion, policy decision, optional activation, indexing, and
success receipt occur in one transaction. A crash cannot expose an Active
memory without its receipt or repeat activation after restart. Retry state
contains only a formed structured candidate, never raw conversation content.
Retries pause while disabled and only candidates originally formed while the
setting was enabled are eligible after re-enable.

Schema migration is backed up and restoration-tested. Existing explicit
capture and historical Vaults remain compatible.

## Governance UI and diagnostics

The ART loopback governance page gains an **Automatic memory** settings area
that clearly states the switch affects every local Agent. It shows the current
setting, policy version, limits, cooldown, Codex/DSH support, last hook result,
last capture result, and pending count. Settings mutation uses the existing
capability session, Origin, CSRF, expiry, and audit controls.

The bound Agent's pending candidates can be inspected with source metadata and
policy reasons, then confirmed, edited-and-confirmed, or rejected by a human.
The UI distinguishes configuration enabled, hook observed, candidate
submitted, and memory activated; installation is never reported as runtime
success.

## Security and privacy

- All host inputs are untrusted data, not instructions.
- No transcript parsing, background transcript replay, independent model
  account, credential capture, or automatic shared-knowledge publication.
- Configuration and receipt storage use owner-only local permissions.
- Loopback UI remains on `127.0.0.1` and retains capability, CSRF, Origin,
  expiry, and no-store protections.
- Failures never block the user's primary Codex task.

## Acceptance boundary

Acceptance requires unit, integration, migration, package, isolated install,
Codex CLI/App, in-app browser, and DSH compatibility evidence. Content-quality
coverage contains at least sixty literal Chinese/English positive, negative,
and malformed cases. A hundred-run non-trigger benchmark must have zero added
model tokens and local hook p95 no greater than 100 ms. The final deliverable
is a candidate plugin and evidence report; automatic memory remains off until
the user enables it, and no public release is performed.
