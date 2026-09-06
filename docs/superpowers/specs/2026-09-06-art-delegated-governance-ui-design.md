# ART Delegated Governance UI

**Status:** Proposed for written operator review

**Target release:** ART 0.3.3

## Purpose

ART 0.3.3 replaces command-line-dependent human governance with one ART-owned
local governance page that Codex Desktop and DSH can display. It also adds an
explicitly marked, persistent, default-off delegated governance mode. When that
mode is enabled for one local Agent identity, one unambiguous user instruction
may authorize the Agent to create a proposal and atomically approve and publish
it without a second command or separate human form.

This mode is intentionally more convenient and less strict than ART 0.3.2
human-only governance. It never mislabels delegated execution as human review.

## Approved product decisions

- Normal human interaction with ART is page-based. Users are never instructed
  to run CLI commands for settings, review, publication, status, or audit.
- ART owns one local governance web application. Codex Desktop opens it in its
  in-app browser; DSH embeds or opens the same application.
- Delegated governance is disabled by default.
- The setting persists for the tuple of local ART host binding and Agent
  identity, so it survives new conversations and host restarts.
- Delegated governance applies to every ART risk level.
- One user instruction may authorize proposal creation, approval, and
  publication. Review and publication do not require two user commands.
- Delegated review is recorded as Agent-delegated execution, never as Human.
- The normal operation is atomic: it materializes the Edition and audit records
  together or leaves the proposal unchanged.
- Existing CLI internals may remain for compatibility, development, and
  disaster recovery, but they are not part of the user or Agent workflow.

## Trust statement

Delegated mode trusts the configured local Agent to interpret the current
user's instruction. ART can prove which bound Agent performed which mutation
against which exact proposal snapshot. ART cannot independently prove the
origin or meaning of the conversational instruction because standard MCP does
not expose an authenticated user-message receipt.

The product states this limitation plainly. Delegated audit records use a
distinct actor class and never imply direct human review. Enabling the mode is
an explicit decision to accept this weaker local trust model.

ART does not infer delegated authority from Codex Full Access, filesystem
permissions, client names, approval policy, or an Elicitation decline. Those
signals are either unavailable to the MCP server or do not prove governance
intent.

## User experience

### First-time configuration

1. The Agent requests an ART governance UI session.
2. Codex Desktop opens the returned loopback URL in its in-app browser. DSH
   renders the same URL in its page surface.
3. The user opens Delegation settings.
4. The page shows the bound Agent, local scope, persistence, actor label, and
   trust statement.
5. The user enables Allow Agent delegated governance.
6. ART persists the setting for the exact host binding and Agent identity and
   appends a bounded configuration audit event.

No terminal command is shown or required.

### Delegated knowledge upgrade

For an instruction such as “把这条记忆升级成知识”:

1. The Agent resolves exactly one source memory from the current context.
2. The Agent creates a proposal from exact authorized revisions.
3. The Agent invokes delegated approve-and-publish with the exact Proposal ID
   and revision.
4. ART verifies delegation, revalidates sources, freezes the proposal snapshot,
   and atomically records delegated approval plus the immutable Edition.
5. The Agent reports the proposal, Edition, actor class, and final state.

The instruction does not need to contain the generated Proposal ID. It must
identify one source or existing proposal unambiguously. If multiple candidates
are plausible, the Agent asks the user to choose and does not call delegated
governance.

### Human governance

When delegation is disabled, Codex Desktop and DSH open the ART page for human
review. The page presents the full proposal, exact revision, source-set hash,
draft hash, decision controls, and publication action. Native MCP Elicitation
may remain available to other compatible clients, but Codex Desktop and DSH
use the ART page as their stable primary surface.

### Page sections

- Pending: proposal state, full content, sources, hashes, and actions.
- Delegation settings: persistent state for the bound Agent.
- Audit: configuration changes, governance actions, Edition references,
  timestamps, actors, and exact snapshot hashes.

The page never exposes a raw private-memory body unless already included in the
proposal draft. It never stores or displays a full chat transcript.

## Architecture

    Codex Desktop / DSH
      -> Agent calls art_governance_ui_open
    ART MCP
      -> creates bounded loopback UI session
      <- 127.0.0.1 URL + expiry + safe session metadata
    Host page surface
      -> loads ART-owned embedded web assets
      -> reads/mutates through session-bound HTTP endpoints
    ART governance service
      -> Knowledge Vault + delegation policy + audit receipts

    Delegated conversation path
    User instruction
      -> bound Agent
      -> art_knowledge_governance(operation=approve_and_publish)
    ART MCP
      -> require persistent delegated policy
      -> freeze/revalidate exact snapshot
      -> atomic delegated approval + publication
      <- Edition receipt

The local UI and MCP tools call the same governance service. Neither surface
writes SQLite state directly.

## Delegation policy

ART stores one policy record per host-binding hash and Agent ID:

    host_binding_hash
    agent_id
    mode = human_review | delegated_local
    enabled_at
    updated_at
    updated_via = local_governance_ui

The default for a missing record is human_review. Configuration lives in
ART-managed local state, not a project repository or Agent-editable skill file.
Disabling delegation affects future calls immediately but never rewrites
historical receipts or published Editions.

## MCP surface

### Open the page

Add art_governance_ui_open with a bounded input:

    {
      "view": "pending | settings | audit",
      "proposal_id": "optional artp_...",
      "revision": 1
    }

It starts or reuses the loopback server, creates a short-lived UI session
scoped to the bound Agent and requested view, and returns URL, expiry, and safe
display metadata. It never returns database paths, secrets, or private source
bodies.

The Codex skill opens the URL in the in-app browser. DSH opens or embeds it in
its page shell. Failure returns a page-launch error with a retry action, never
CLI instructions.

### Delegated governance

Extend art_knowledge_governance.operation with approve_and_publish. The
Agent-supplied input remains limited to:

    {
      "operation": "approve_and_publish",
      "proposal_id": "artp_...",
      "revision": 1
    }

The Agent cannot supply actor, review reason, content hash, risk override, or
publication confirmation. ART obtains identity and hashes from the bound
runtime and canonical proposal state. The fixed audit basis is
current_user_instruction; it is a declared Agent basis, not verified human
identity. Existing review and publish Elicitation operations remain compatible.

## Atomic domain operation

Add a Knowledge Vault operation equivalent to:

    approve_and_publish_delegated_exact(
      governance_snapshot,
      delegated_agent_actor,
      next_edition_number
    ) -> Edition

One transaction must:

1. compare proposal ID, revision, status, source-set hash, and draft hash;
2. verify proposal eligibility and source currency;
3. recheck delegated policy inside the mutation boundary;
4. allocate the next Edition number without a race;
5. append a delegated approval receipt;
6. materialize the immutable Edition and manifest;
7. mark the proposal materialized;
8. append the delegated publication receipt;
9. commit.

Any validation, serialization, manifest-preparation, or database failure before
commit rolls back the operation. No approved-but-unpublished intermediate
proposal state is visible. If the durable Edition commits but its filesystem
projection fails afterward, ART reports a materialized Edition with pending
recovery and uses the existing bounded recovery protocol; it never rewinds the
audit history or exposes the proposal as merely approved.

Concurrent replay returns an already bound Edition only when the complete
idempotency identity matches; otherwise it returns a conflict with no write.

## Actor and audit model

Add an actor representation that cannot be confused with Human:

    AgentDelegated {
      agent_id,
      host_binding_hash,
      authorization_basis = current_user_instruction
    }

Audit records include proposal and Edition references, exact hashes, bound
Agent, host binding, operation, observed policy, bounded task/session ID when
available, timestamps, and terminal outcome. They exclude raw prompts, full
transcripts, secrets, unrestricted metadata, and private source bodies.

## Local web application

The UI is embedded as static assets in the ART binary, avoiding Node.js and
external web dependencies. ART serves it only on a random loopback port.

Each launch creates a high-entropy, short-lived session capability bound to the
Agent, host binding, requested view, optional proposal revision, issue time,
expiry, and one browser session. State-changing requests require the session
capability, same-origin checks, and anti-CSRF state. Sessions expire after a
short idle period and die with the ART process. The server never binds a LAN or
public interface.

These measures prevent accidental cross-origin and stale-page mutations. They
do not claim to prevent a trusted Full Access Agent from automating the local
page; delegated mode intentionally accepts that local trust boundary.

The UI must support keyboard operation, visible focus, semantic labels, status
text independent of color, reduced motion, and the narrow Codex side panel.

## Host integration

### Codex Desktop

The ART skill opens a successful art_governance_ui_open URL in the in-app
browser. The user remains inside the same desktop task and never opens
Terminal. When delegated mode is enabled, routine knowledge upgrades do not
open the page; the page remains available for settings, pending work, and
audit.

### DSH

The DSH integration consumes the same open result and displays the ART page in
its existing browser/page container. DSH does not duplicate policy or
governance mutations.

### Other clients

Clients without a page surface receive PAGE_SURFACE_REQUIRED. Agent-facing
instructions provide no command-line fallback.

## Failure behavior

- Delegation disabled: zero writes; return DELEGATION_DISABLED and page action.
- Ambiguous target: the skill stops before proposal creation.
- Proposal or source drift: zero writes; return SOURCE_STALE.
- Invalid proposal state: zero writes; return INVALID_GOVERNANCE_STATE.
- Concurrent policy disable: the transaction rechecks policy; zero writes.
- Competing Edition allocation: no partial review; retry a fresh snapshot.
- Page unavailable: return GOVERNANCE_UI_UNAVAILABLE with retry, not CLI.
- Expired or reused page session: zero writes; issue a fresh session.
- Browser close or navigation: no mutation unless the server committed it.
- Partial projection after commit: use existing bounded recovery and show it
  on the audit page.

## Compatibility and migration

- Existing memory, proposal, Edition, retrieval, and Elicitation records remain
  readable.
- A schema migration adds delegation policy and delegated actor/audit support.
- Missing policy rows implicitly mean human_review.
- Existing CLI operations remain binary-compatible only for maintainers and
  automated recovery; skills, normal docs, errors, and UI never route users
  there.
- Codex and DSH plugin payloads and acceptance documents move together.

## Repository governance contract

The current repository instruction “Agents never approve or publish knowledge”
describes the ART 0.3.2 trust model and conflicts with this approved feature.
Implementation must replace it with a precise rule: Agents cannot perform
human governance, but a bound Agent may perform distinctly labeled delegated
governance only when the persistent local policy is enabled. This contract
change must land with the domain enforcement and tests, never as a
documentation-only relaxation.

## Testing strategy

Development follows red-green-refactor with task-owned ART_HOME directories.

### Domain and storage

- default-off policy, identity isolation, persistence, and immediate disable;
- every risk level is eligible after enablement;
- AgentDelegated can never deserialize or render as Human;
- atomic operation exposes no approved intermediate state;
- injected failures, replay, concurrency, drift, policy disable, and competing
  Edition allocation preserve consistency.

### MCP

- strict schema for art_governance_ui_open;
- approve_and_publish exposes no decision, actor, reason, risk, or confirm;
- disabled delegation creates zero knowledge writes;
- enabled delegation creates exactly one Edition and linked delegated audit;
- existing Elicitation contracts remain green.

### Web application

- loopback-only random port, session expiry, same-origin, CSRF, stale revision,
  and replay contracts;
- bounded redacted Pending, Settings, and Audit APIs;
- keyboard, focus, semantic-label, reduced-motion, responsive-layout, refresh,
  browser-close, and expiry checks.

### Installed-host acceptance

- install the candidate in isolated Codex and DSH homes;
- open the real ART page inside both page surfaces;
- enable delegation, restart each host, and prove persistence;
- issue one natural-language memory-to-knowledge instruction;
- verify one proposal, delegated approval, one Edition, correct actor, and
  visible audit;
- disable delegation and prove a later Agent call cannot publish;
- prove no normal user journey or Agent response requires Terminal;
- verify source, package, installed runtime, live MCP, browser UI, and durable
  state as separate evidence layers.

## Documentation changes

Update the ART skill, README, operations guide, security model, changelog,
plugin metadata, schemas, and acceptance artifact for page-first interaction,
persistent delegated mode, single-instruction atomic publication, the distinct
AgentDelegated identity, the weaker trust boundary, and removal of CLI from
normal workflows.

## Acceptance criteria

ART 0.3.3 is complete only when:

1. Codex Desktop and DSH open the same ART-owned local page.
2. A user configures delegation once without Terminal.
3. The setting persists for the exact local Agent identity.
4. One unambiguous instruction creates, approves, and publishes one Edition.
5. Every mutation is labeled AgentDelegated and bound to exact hashes.
6. The operation is atomic with no visible half-completed state.
7. Disabling delegation blocks subsequent publication immediately.
8. Human review remains available through the page when delegation is off.
9. Existing memory, recall, proposal, Elicitation, backup, and recovery remain
   compatible.
10. No normal Codex or DSH journey instructs the human to use CLI.
