# ART MCP Elicitation Governance

**Status:** Approved by the operator on 2026-09-06

**Target release:** ART 0.3.2

## Purpose

ART currently lets an Agent create a sourced Knowledge Proposal, but only the
operator CLI can review and publish it. ART 0.3.2 will let a supporting MCP
client collect those human decisions in its own user interface while
preserving the central governance rule:

- an Agent may request a decision;
- only a user action returned by the MCP client may supply that decision;
- the Agent can never pass an approval, rejection, publication confirmation,
  reviewer identity, or review reason as tool arguments.

The CLI remains the complete fallback and recovery surface. Existing private
memory, proposal, Edition, backup, and retrieval formats do not change.

## Selected approach

Add one seventh Agent-safe MCP tool, `art_knowledge_governance`. It has two
operations, `review` and `publish`. Each operation loads and freezes the exact
proposal state, verifies that the connected MCP client advertises form
elicitation, issues one server-to-client elicitation inside the originating
tool call, and mutates state only after validating the returned user response.

This is preferred over extending `art_knowledge_propose`, because proposal
creation and human governance have different authority and retry semantics. It
is preferred over interpreting chat text, because raw messages are visible to
the Agent and are not an independently attributable review event.

The MCP surface intentionally changes from exactly six to exactly seven tools.
The original six tools retain their names, inputs, outputs, and permissions.

## Product flow

```text
Agent
  -> art_knowledge_propose(exact private revisions)
  <- submitted proposal id, revision, source-set hash

Agent
  -> art_knowledge_governance(operation=review, proposal id, revision)
ART MCP server
  -> verify proposal and client elicitation capability
  -> elicitation/create(review form bound to exact proposal snapshot)
MCP client
  -> show the proposal and structured choices to the local user
User
  -> approve | request changes | reject, with a reason
ART MCP server
  -> re-read and compare the proposal snapshot
  -> append the human review receipt
  <- reviewed status

Agent
  -> art_knowledge_governance(operation=publish, proposal id, revision)
ART MCP server
  -> require an approved exact revision
  -> elicitation/create(separate publication confirmation)
User
  -> explicitly confirm or decline
ART MCP server
  -> re-read and compare the approved snapshot
  -> publish one immutable Edition
  <- Edition reference and hashes
```

Review and publication are always separate tool calls and separate human
interactions. A review response can never implicitly publish.

## MCP contract

### Tool input

`art_knowledge_governance` accepts only:

```json
{
  "operation": "review | publish",
  "proposal_id": "artp_...",
  "revision": 1
}
```

The schema rejects unknown fields. In particular it has no `decision`,
`confirm`, `reason`, `actor`, or free-form command field. This makes a forged
Agent tool call insufficient to approve or publish knowledge.

### Review elicitation

The server loads the proposal and constructs a form-mode elicitation that
contains:

- Proposal ID and exact revision;
- knowledge key, title, applicability, sensitivity, and risk;
- the complete proposed Markdown body;
- source-set hash and a deterministic draft hash;
- choices `approve`, `request_changes`, and `reject`;
- a required bounded human reason.

The response schema is a flat MCP-compatible object. ART accepts the response
only when the client returns `action=accept`, the decision is one of the three
allowed values, and the reason is non-empty and within the documented bound.
`decline` and `cancel` do not change proposal state.

Normal-risk proposals may be approved through one elicitation. Existing
elevated/high-risk independent-review rules remain unchanged. ART 0.3.2 does
not claim that an unauthenticated stdio client can distinguish two humans;
when another independent reviewer is required, the tool returns a bounded
operator-action result and leaves the proposal under review. The CLI or a
future authenticated operator adapter remains necessary for that second
identity.

### Publish elicitation

Publication is available only for an exact approved revision whose review
receipt still matches its source-set hash. The elicitation displays the same
identity and hash fields plus the next immutable Edition number. Its form has
one required boolean confirmation and a clear statement that publication
creates shared immutable knowledge.

Only `action=accept` with `confirm=true` may call the existing atomic publish
path. `false`, `decline`, `cancel`, timeout, disconnect, malformed content, a
changed proposal, or an invalid state causes no publication.

### Tool output

All paths return bounded machine-readable data under `art.mcp.v1`, including:

- operation and exact proposal reference;
- `outcome=reviewed|published|declined|cancelled|operator_action_required`;
- the resulting proposal status or Edition reference when applicable;
- a stable, non-secret reason code for unsupported or interrupted flows.

No response includes private memory bodies, raw source anchors, unrestricted
client metadata, or a conversation transcript.

## Capability negotiation and compatibility

The server enables rmcp's `elicitation` feature and inspects the connected
client capabilities recorded during MCP initialization. It issues a form
elicitation only when form mode is advertised. Empty or missing elicitation
capabilities are treated as unsupported; ART does not guess from a client name
or protocol version.

Unsupported clients receive `outcome=operator_action_required` with a stable
`ELICITATION_UNSUPPORTED` reason and the exact proposal reference. ART does not
attempt to interpret later chat text and does not mutate state. The skill tells
the Agent to guide the user to the operator CLI without executing the review or
publication command itself.

The existing CLI continues to support proposal inspection, approval, change
requests, rejection, publication, and recovery. Codex and DSH remain supported
when they do not advertise elicitation; their existing memory and recall flows
are unaffected.

## Trust model

ART treats the local MCP client as the user-interaction boundary, just as the
operator CLI currently treats the local terminal as that boundary. An accepted
elicitation is attributable to a user interaction performed by that client,
not to the Agent that requested it.

The recorded review actor is a stable local elicitation actor derived from the
ART host binding and bounded MCP client implementation metadata. ART never
accepts an actor string from the Agent or from elicitation form content. The
record does not claim a globally authenticated personal identity.

The design protects against:

- an Agent supplying its own `approve` or `confirm` argument;
- replaying an approval against another Proposal or revision;
- changing a Proposal between display and commit;
- combining review and publication into one consent;
- treating cancellation, timeout, disconnect, or malformed content as consent;
- silently weakening multi-human requirements for elevated knowledge.

A malicious or compromised MCP client is outside this local trust boundary.
Future remote or multi-user operation requires authenticated operator identity
and is not implied by stdio Elicitation.

## State and consistency

Before elicitation, ART captures a snapshot containing proposal ID, revision,
status, source-set hash, and draft hash. After the user responds and before any
write, ART re-reads the proposal and requires every snapshot field to match.
Mismatch returns a stale-state outcome and requires a new review interaction.

The existing Knowledge Vault remains the only component allowed to append
review receipts or publish Editions. MCP owns interaction and validation, not
knowledge persistence. No database schema migration is required unless the
implementation review proves that existing review receipts cannot represent
the bounded elicitation actor; any such finding must stop implementation and
upgrade this design.

## Failure behavior

- Unsupported form elicitation: return `ELICITATION_UNSUPPORTED`; no write.
- User decline: return `declined`; no write.
- User cancel or dismissal: return `cancelled`; no write.
- Timeout or client disconnect: return a retryable interruption; no write.
- Missing or malformed response content: return validation failure; no write.
- Blank or oversized reason: return validation failure; no write.
- Proposal revision, status, source-set hash, or draft hash drift: return
  `SOURCE_STALE`; no write.
- Review of elevated/high-risk content without a distinct second reviewer:
  preserve `under_review` and return `operator_action_required`.
- Publication without an exact approval or without `confirm=true`: fail closed;
  no publish intent is created.
- A crash after publication begins continues to use the existing recoverable,
  atomic publish-intent protocol.

## Skill behavior

The Codex and DSH skills are updated together:

1. Create proposals only from exact authorized private revisions.
2. Prefer `art_knowledge_governance` when the user asks to review or publish.
3. Explain that the upcoming prompt is generated by ART and must be answered by
   the user in the client UI.
4. Never infer consent from ordinary chat text and never execute operator CLI
   review or publication commands.
5. When Elicitation is unsupported, report the exact Proposal reference and
   provide operator CLI guidance as a user action.

## Versioning and packaging

All workspace packages, plugin manifests, launch documentation, release
manifests, installer checks, and version tests move from 0.3.1 to 0.3.2. The
plugin launcher introduced in 0.3.1 remains unchanged except for any versioned
acceptance references.

The changelog must call out the seventh tool and the fact that Elicitation is a
capability-negotiated convenience path, not a removal of human-only governance
or the operator CLI.

## Test strategy

Development follows red-green-refactor with task-owned temporary ART homes.

### Knowledge-domain contracts

- existing Agent actors still cannot review;
- elicitation uses a Human actor generated outside Agent input;
- source/revision drift blocks review and publication;
- elevated/high-risk independent-review behavior remains unchanged;
- publication stays atomic and requires an approved matching source set.

### MCP contracts

- the tool surface is exactly seven and all original schemas are unchanged;
- the governance input schema exposes no decision, actor, reason, or confirm;
- a form-capable test client receives exact review fields and can approve,
  request changes, or reject;
- review and publish require separate elicitation requests;
- unsupported capability, decline, cancel, timeout, disconnect, malformed
  content, blank reason, and stale snapshot all cause zero writes;
- publish confirmation produces exactly one immutable Edition and replay cannot
  create a second Edition;
- no private source body or client-supplied identity enters shared output.

### Plugin and host contracts

- restricted-PATH startup and clean EOF/reconnect coverage remain green;
- Codex and DSH skills name the exact seven tools and preserve CLI fallback;
- an isolated real Codex session discovers the 0.3.2 plugin and original six
  tools without regression;
- if that Codex build advertises form Elicitation, exercise a real human review
  and separate publication against disposable data;
- if it does not advertise Elicitation, prove the exact unsupported fallback in
  real Codex and run the full accept/decline/cancel protocol against the rmcp
  test client. Do not claim native Codex Elicitation support in that case.

### Release gates

- formatting, locked workspace tests, Clippy with warnings denied;
- release-version, plugin lifecycle, source identity, credential/path, license,
  backup/recovery, migration, stress, and performance gates;
- isolated candidate archive and installed-plugin launch acceptance;
- final `art doctor` against disposable and formal read-only homes;
- review of task-owned processes, temporary homes, build targets, archives, and
  logs before completion.

## Acceptance criteria

ART 0.3.2 is ready only when:

1. Agents can request but cannot encode a governance decision.
2. A supporting MCP client can review and publish without a terminal.
3. Every user decision is bound to the exact displayed Proposal revision and
   hashes.
4. Review and publication require distinct user interactions.
5. Unsupported clients fail closed and retain the CLI fallback.
6. The existing storage, retrieval, isolation, recovery, and publication
   guarantees pass unchanged.
7. Local Codex acceptance states precisely whether native Elicitation worked or
   only the compatibility fallback was available.

## Non-goals

- interpreting an ordinary chat reply as approval;
- exposing direct Agent-controlled approve or publish tools;
- replacing the operator CLI;
- adding a browser, web service, AAA integration, or remote review authority;
- authenticating multiple human identities through local stdio client metadata;
- changing private-memory assurance, recall ranking, backup, or Edition formats.
