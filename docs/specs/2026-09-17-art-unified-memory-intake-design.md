# ART Unified Private-Memory Intake Design

**Status:** Approved for local candidate implementation

**Target:** post-0.3.5 local trial; no public release

## Purpose

ART keeps two separately governed object families: private Agent memories and
shared Knowledge Editions. This change affects only private memory intake.

Private memory accepts three origins through one policy and transaction path:

- `user_requested`: the current Agent states that the user explicitly asked to
  remember the conclusion. This remains available while automatic memory is
  disabled. It is an auditable Agent assertion, not cryptographic host proof.
- `agent_initiated`: the Agent independently identifies a bounded, reusable,
  sourced conclusion during ordinary task work.
- `hook_triggered`: the Codex Stop Hook prompts one bounded continuation, and
  the current Agent independently decides whether a memory is worthwhile.

Agent-initiated and Hook-triggered intake are automatic memory. Both require
the machine-wide switch, share admission limits, and use the same value
standard. The Hook is one trigger mechanism and is not a prerequisite for an
Agent to submit worthwhile memory.

## Unified value standard

A worthwhile private memory is bounded, reusable, sourced, and likely to help
a future task. Appropriate subjects include a durable decision and rationale,
an applied user correction, a stable project fact, a verified repair, or a
repeatable operating procedure. It states scope, evidence date, applicability,
and material uncertainty.

Ordinary progress, greetings, transient state, unresolved failures,
speculation, recalled text presented as new evidence, duplicate conclusions,
secrets, and raw conversation are not worthwhile memory.

The same versioned guidance is used by the ART skill for ordinary Agent work
and by the Stop Hook continuation. Hook keyword classification remains only a
cheap reminder filter; it does not define the value standard.

## Intake contract

The MCP surface retains `art_memory_capture` and
`art_memory_candidate_submit` for compatibility, but both delegate to one
Vault intake service.

`art_memory_capture` adds:

- `capture_origin`: `user_requested` or `agent_initiated`; an omitted value is
  treated as `agent_initiated`.
- `request_basis`: required for `user_requested`; a short sanitized basis, not
  a full prompt.
- `value_reason`: required for `agent_initiated`; the future use and reusable
  value of the conclusion.
- optional session and turn references for diagnostics, shared budgeting, and
  cross-trigger deduplication. References state whether the host supplied or
  the Agent asserted them.

`art_memory_candidate_submit` remains the Hook adapter and requires its exact
Hook trigger receipt. Callers cannot supply identity, state, assurance,
reviewer, or policy actor.

Both tools return a unified disposition: `activated`, `pending_review`,
`duplicate`, `rejected`, `disabled`, or `rate_limited`, plus a processing
receipt, origin, reason, policy/config versions, replay status, and a memory
reference only when one was stored or matched.

## Policy and lifecycle

All origins share structure, sensitivity, scope, provenance, evidence,
duplicate, and conflict checks. User request does not exempt sensitive content
or malformed evidence.

`user_requested` content may activate immediately after checks. Uncertain or
conflicting content remains Candidate. `agent_initiated` and `hook_triggered`
content is Candidate-first; verifiable, non-conflicting content may activate
inside the same transaction, otherwise it remains pending. Sensitive content
is rejected without storing the body.

Automatic revision requires the exact target memory revision. It creates a
pending revision proposal while preserving the current Active revision. Human
confirmation rechecks the expected revision before replacement. Explicit
user-requested revision may update after checks and version match; uncertainty
still produces a pending revision.

The automatic switch is rechecked before admission and before commit. A
completed disable prevents later automatic storage or activation. Existing
memories, recall, explicit user-requested intake, and human governance remain
available.

## Budget, idempotency, and recovery

Agent-initiated and Hook-triggered intake share the configured default budget
of three admissions per Agent/session and a ten-minute cooldown. A Hook claim
and its subsequent candidate submission consume one admission. Same-key retry,
exact duplicate detection, and a reliably linked ordinary-turn submission do
not double-count. A Hook skips continuation when a reliably linked turn was
already handled.

When a host cannot provide stable session identity, ART uses a persistent
Agent/day fallback bucket and exposes degraded attribution in diagnostics.
Process restart does not reset that budget.

One unified submission receipt records origin, attribution confidence,
payload hash, disposition, evidence hash, budget association, policy/config
versions, and time. Same Agent/key/content retries return the prior result;
same key with different content conflicts. Memory rows, revisions, index,
policy decision, and success receipt commit atomically.

Migration backs up and restores existing Vaults. Historical capture receipts
remain valid and gain `legacy_unspecified` origin without guessing user intent.
No raw transcript is stored or replayed.

## Governance and diagnostics

The governance page describes the global control as covering Agent-initiated
and Hook-triggered automatic memory on the machine. It shows each origin
separately, shared budget state, host support, attribution confidence, recent
dispositions, and pending counts. Historical origin is shown as unknown.

Pending candidates and revision proposals show source metadata, value reason,
policy reason, and the original/current content needed for review. Human
confirm, edit-confirm, and reject retain existing loopback authentication,
Origin, CSRF, expiry, and audit protections.

## Acceptance boundary

Acceptance requires unit, integration, migration/restore, concurrency,
packaging, isolated install, Codex CLI/App, in-app browser, and DSH regression
evidence. At least sixty bilingual value cases cover both ordinary Agent and
Hook guidance. One hundred non-trigger Hook runs add no model continuation and
meet local p95 <=100 ms.

The final candidate is installed into local Codex with the user's prior switch
value preserved. Formal user Vault data is not used for automated fixtures.
The work stops after local trial acceptance; it does not tag, push, or publish.
