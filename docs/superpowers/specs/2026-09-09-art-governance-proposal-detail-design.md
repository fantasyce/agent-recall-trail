# ART 0.3.5 Knowledge Proposal Detail and Human Review Design

**Status:** Approved for implementation; no implementation started

**Target release:** ART 0.3.5

## Purpose

ART 0.3.4 lets a human review and publish knowledge proposals from an ART-owned
local page, but each pending card exposes only a short summary. A reviewer can
therefore reach a governance decision without seeing the exact knowledge body,
its locked sources, its relationship to the current published Edition, or the
complete review history.

ART 0.3.5 will turn that queue into a complete review workspace. A reviewer
must open the exact proposal revision, inspect its full content and evidence,
and make every human governance decision inside the detail view. Codex Desktop
and DSH continue to use the same loopback web application; the user is never
sent to a command-line workflow.

## Approved product decisions

- Desktop uses a right-side review drawer. Viewports narrower than 760 px use a
  full-screen detail surface.
- Queue cards remain concise and expose one primary action: **Review details**.
- Approve, request changes, reject, and publish are available only inside the
  detail surface. The queue does not retain shortcut decision buttons.
- The default content view is safely rendered Markdown. A separate tab exposes
  the exact raw Markdown.
- When a current Edition exists for the same knowledge key, the detail surface
  includes a line-level comparison. A first Edition clearly states that there
  is no earlier version.
- Human review sessions have one fixed 30-minute lifetime. This setting is not
  configurable in ART 0.3.5.
- The page supports the complete human review loop: approve, request changes,
  reject, and publish.
- Human review and publication remain distinct visible decisions. After an
  approval, the same detail surface changes to an explicit publication step.
- The existing `human_review` governance boundary remains unchanged. ART does
  not infer human authority from Codex Full Access, Agent permissions, or page
  automation.
- Normal human interaction remains page-based in Codex Desktop and DSH. CLI
  commands are not presented as the ordinary review or publication path.

## User experience

### Pending queue

Each pending card shows only information needed to decide what to inspect next:

- title and applicability;
- proposal status, sensitivity, and risk;
- proposal revision and last-updated time;
- one **Review details** button.

The queue header shows the actual actionable proposal count. It must not use a
static label such as `QUEUE / 02`.

The actionable queue contains proposals in `submitted`, `under_review`, and
`approved` states. Materialized and rejected proposals do not remain in the
pending queue, although their receipts remain available through ART's audit
surfaces.

When `art_governance_ui_open` creates a session for an exact Proposal ID and
revision, the page loads and opens that proposal automatically. A general
pending session shows the complete actionable queue without selecting an item.

### Detail surface

On desktop, opening a proposal reveals a native dialog presented as a right
drawer with width `min(880px, 72vw)`. Below 760 px, it occupies the full
viewport. The header and action footer remain visible while the body scrolls.
The main reading column is capped near 72 characters per line.

The surface preserves ART's restrained paper, ink, and moss visual language.
It uses a provenance spine rather than decorative dashboard chrome:

    Draft -> Sources -> Decision -> Publication

The header contains:

- title, proposal ID, and exact revision;
- status, sensitivity, and risk labels whose meaning is not color-only;
- author and creation/update timestamps;
- draft hash and source-set hash;
- close control and remaining session time.

The body contains three content views:

1. **Knowledge** — sanitized rendered Markdown for the complete Applicability
   and Knowledge body.
2. **Raw Markdown** — the exact canonical Markdown in a readable code surface.
3. **Changes** — a grouped line diff against the current Edition, with three
   context lines. For a first Edition, this view explains that all content is
   new and no comparison target exists.

The content views are followed by:

- locked source records with source type, ID, revision, content hash, source
  anchor, and applicable grant hashes;
- current Edition metadata when one exists;
- review history with decision, time, channel, and reason;
- governance requirements, including any independent-review requirement;
- the actions currently allowed for the exact status.

The page never exposes a raw private-memory body or a chat transcript merely
because the proposal references it. Source records prove identity and locking;
the proposal draft remains the content under review.

### Review decisions

For a `submitted` proposal, the detail footer offers:

- **Approve**;
- **Request changes**;
- **Reject**.

Selecting a decision opens an inline form within the drawer. A reason is
required and must contain between 1 and 1,000 characters after trimming. The
form repeats the exact Proposal ID and revision before submission.

An `under_review` proposal explains what additional review is required. For
elevated or high-risk knowledge, existing independent-review rules remain in
force. ART 0.3.5 does not invent a new authenticated human identity system; if
the local page cannot satisfy a distinct-reviewer requirement, it must explain
that limitation rather than weakening the rule.

A rejected proposal is terminal for that revision. It is removed from the
actionable queue after the success receipt is shown. A future replacement must
be a new proposal or a valid new revision produced through existing domain
rules.

### Publication

After approval, the footer changes to a publication action. Publication uses an
inline confirmation panel rather than an immediate button mutation. The panel
shows:

- exact Proposal ID and revision;
- draft and source-set hashes;
- the predicted next Edition number;
- a statement that the resulting Edition is immutable;
- a separate explicit **Publish Edition** confirmation.

On success, the drawer retains a publication receipt containing Edition ID,
Edition number, publication time, Markdown hash, and manifest hash. The queue
removes the now-materialized proposal. Closing the receipt returns the user to
the refreshed queue.

### Expiry and conflicts

The page displays a non-disruptive warning when five minutes remain. At
expiration, loss of the local server, or authorization failure, it enters a
persistent expired state:

- all mutation controls are disabled;
- entered review text remains visible so the user can copy it;
- the page asks the user to have the Agent reopen ART governance;
- it does not suggest CLI commands.

Revision, status, source-set, or draft drift produces a conflict view. The page
does not silently reload and submit against new content. It disables the stale
decision and asks the reviewer to reopen the current revision.

## Architecture

    Codex Desktop / DSH
      -> Agent calls art_governance_ui_open
    ART MCP
      -> creates a 30-minute loopback capability session
      <- local review URL
    Governance queue
      -> summary-only bootstrap
      -> exact read-only proposal-detail request
    Detail drawer
      -> sanitized Markdown / raw Markdown / Edition diff / source locks
      -> exact review mutation
      -> exact publication mutation
    Knowledge Vault
      -> validates revision, hashes, status, review rules, and Edition number
      -> records review or materializes immutable Edition

The local web application remains embedded in the ART binary. It uses no CDN,
remote images, Node.js service, or public network listener. UI routes call the
same Knowledge Vault domain operations used by the MCP layer and never write
SQLite or Edition files directly.

## HTTP and data contracts

### Existing public surface

The public MCP tool count and `art_governance_ui_open` input contract remain
unchanged. No database migration is required solely for this feature.

`GET /api/bootstrap` remains summary-only. It adds the real actionable count
and preserves the session's optional target Proposal ID and revision so the
client can open the exact requested item. It must not include full draft bodies
or private source content.

### Proposal detail

Add the read-only endpoint:

    GET /api/proposal-detail?session=<capability>&proposal_id=<id>&revision=<n>

Its response uses schema identifier:

    art.governance.proposal-detail.v1

The response contains:

- exact proposal metadata and status;
- canonical Applicability and Knowledge Markdown;
- sanitized rendered HTML derived from that Markdown;
- draft hash and source-set hash;
- locked, non-secret source metadata;
- ordered review records;
- current Edition metadata and verified canonical Markdown, when present;
- grouped line diff data;
- predicted next Edition number;
- currently allowed actions and outstanding review requirements.

The server authorizes the capability before resolving a proposal. A session
created for one exact proposal and revision cannot read or mutate another.
General pending sessions may read the proposals returned in their authorized
queue.

`art-knowledge` adds controlled read methods for proposal review records and
the verified Markdown of the current Edition. Edition files are accepted only
after their stored hashes are revalidated; a projection path alone is not
trusted.

### Review response

`POST /api/review` continues to require the session capability, CSRF token,
Proposal ID, revision, decision, and reason. It accepts the existing approval
and change-request decisions plus `rejected`.

The response returns the authoritative updated proposal status and a bounded
review receipt. It never treats a client-side optimistic state as proof that a
review committed.

### Publication response

`POST /api/publish` continues to require the exact snapshot and explicit
confirmation. A successful response adds Edition ID, Edition number,
publication time, Markdown SHA-256, and manifest SHA-256 for the receipt view.

## Rendering and comparison

Use Rust-native dependencies in the ART binary:

- `pulldown-cmark` 0.13.4 for Markdown parsing;
- `ammonia` 4.1.4 for HTML sanitization;
- `similar` 3.2.0 for canonical line comparison.

Rendered Markdown permits ordinary semantic prose, headings, lists, tables,
code, and links. Sanitization removes scripts, event handlers, embedded styles,
iframes, active content, and remote images. Links are limited to safe protocols
and receive `noopener` and `noreferrer` behavior when opened outside the page.
Raw HTML from proposal Markdown is never trusted as page markup.

The comparison input is the canonical Applicability plus Knowledge content,
not the generated Edition front matter. Diff grouping uses three unchanged
context lines. Additions, removals, and unchanged lines have text labels and
semantic markup so the comparison is understandable without color.

## Security and privacy

Preserve all existing controls:

- bind only to a random `127.0.0.1` port;
- use a high-entropy session capability and anti-CSRF token;
- validate Origin for mutations;
- bind the session to Agent identity, host binding, requested view, and any
  exact proposal revision;
- compare proposal revision, status, draft hash, and source-set hash at the
  mutation boundary;
- fail closed without a write on expired, stale, or mismatched requests.

All governance HTML and API responses add:

    Cache-Control: no-store
    Referrer-Policy: no-referrer
    X-Content-Type-Options: nosniff

The detail API does not return secrets, database paths, raw prompts, full
transcripts, or unrestricted private-memory bodies. A decision made through
this page is recorded through the existing local governance actor model; the
UI must not claim stronger authentication than ART actually possesses.

## Accessibility and responsive behavior

- Use native dialog semantics with an accessible name and description.
- Move focus into the drawer when opened and restore it to the originating
  queue card when closed.
- Keep focus trapped within the open modal and provide a visible focus ring.
- Support keyboard tab selection and Escape dismissal when no mutation is in
  progress or confirmation would be lost.
- Announce load, decision, conflict, expiry, and publication results through an
  appropriate live region.
- Make statuses and diffs understandable without relying on color.
- Honor `prefers-reduced-motion`; the drawer may appear without translation.
- Preserve readable controls and content at the narrow widths used by Codex
  Desktop's in-app browser and DSH page surfaces.

## Expected implementation map

The implementation is expected to remain focused in these areas:

- `crates/art-mcp/src/governance_ui.rs` — session lifetime, detail endpoint,
  authorization, response contracts, security headers, and mutation receipts.
- `crates/art-mcp/assets/governance/index.html` — semantic drawer, tabs,
  decision forms, receipt, expiry, and conflict surfaces.
- `crates/art-mcp/assets/governance/app.js` — queue/detail state machine,
  exact reads, review/publication submissions, focus management, and expiry.
- `crates/art-mcp/assets/governance/styles.css` — desktop drawer, narrow-screen
  full-page layout, content typography, provenance spine, diff, and states.
- `crates/art-knowledge/src/lib.rs` — controlled review-history and verified
  current-Edition reads, plus rejection support if not already exposed by the
  domain API.
- focused `art-mcp` and `art-knowledge` tests — contract, security, state, and
  rendering coverage.
- version, changelog, packaging, plugin, website, release-document, and
  acceptance records that must agree on 0.3.5.

The implementation should follow test-driven development. This design does not
authorize unrelated refactoring of the governance server, Knowledge Vault, or
plugin packaging.

## Verification plan

### Contract and domain tests

Add focused tests proving:

- bootstrap responses never contain full proposal bodies or raw private
  sources;
- detail reads return only the exact authorized proposal revision;
- exact-target sessions cannot read a second proposal;
- invalid, expired, stale-revision, stale-status, draft-hash, and source-hash
  cases fail without a write;
- review history is ordered and contains the committed decision basis;
- approve, request changes, reject, and publish return authoritative status and
  receipts;
- rejection is terminal for the reviewed revision;
- elevated/high-risk independent-review requirements remain enforced;
- a first Edition and a proposal replacing a current Edition generate the
  correct comparison model;
- current Edition Markdown is hash-verified before comparison or display.

### Rendering and browser tests

Use malicious Markdown fixtures to verify that scripts, JavaScript URLs, event
handlers, raw active HTML, iframes, styles, and remote images cannot execute or
leak the session capability.

Run browser acceptance against a task-owned ART Home and task-owned proposals:

- desktop drawer and sub-760 px full-screen layout;
- long rendered knowledge and raw Markdown;
- first-Edition and current-Edition diff views;
- source provenance and review history;
- keyboard traversal, focus restoration, Escape behavior, live announcements,
  visible focus, and reduced motion;
- approve, request changes, reject, and publish journeys;
- five-minute warning, expiry, reconnect failure, and stale conflict states;
- exact proposal auto-open from both Codex Desktop and DSH launch paths.

### Repository and release checks

Run the complete workspace unit/integration suite, formatting, linting,
open-source checks, dependency/license review, packaging checks, and version
consistency checks. Build the 0.3.5 candidate from the final source and verify
the embedded governance assets, rather than testing only files from the source
checkout.

Final acceptance must use the candidate installed into real local Codex and
DSH environments while all fixtures and mutable governance state remain in a
task-owned ART Home. Acceptance must prove the full page workflow, not merely
HTTP responses or screenshots.

After acceptance, remove task-created proposals, browser sessions, temporary
ART Homes, build intermediates, attributed listeners, and other test residue.
Report the final Git state, retained evidence, and any skipped coverage.

## Version and release obligations

The release changes every authoritative version surface to 0.3.5, including
Cargo metadata, plugin manifests and installation metadata, changelog and
README references, website/release metadata, consistency tests, and the final
acceptance record. Exact files must be resolved from the repository at
implementation time rather than inferred from this design.

The release record must include a dedicated 0.3.5 design/acceptance artifact
and evidence from both Codex Desktop and DSH using the final installed bytes.

## Non-goals

ART 0.3.5 does not add:

- bulk approval or bulk publication;
- remote or LAN-accessible governance pages;
- CLI instructions as a normal user workflow;
- a new authenticated human identity provider;
- disclosure of private source bodies or transcripts;
- Agent-side automatic review or publication in `human_review` mode;
- configurable governance-session duration;
- content editing inside the review drawer;
- changes to delegated governance semantics.

Duplicate or related proposals continue to be reviewed one at a time. A
rejected proposal does not silently supersede, merge, or delete another
proposal.

## Acceptance criteria

ART 0.3.5 is ready only when all of the following are true:

1. A reviewer cannot approve, request changes, reject, or publish from the
   summary queue.
2. The detail surface shows the complete exact proposal revision, safe rendered
   content, raw Markdown, source locks, hashes, history, and applicable Edition
   comparison before exposing decisions.
3. All review and publication mutations remain exact, conflict-safe, and
   fail-closed.
4. The local page works with keyboard-only input and in both desktop-drawer and
   narrow full-screen layouts.
5. No malicious Markdown can execute active content, access the session
   capability, or trigger a remote image request.
6. Human review and immutable publication complete without asking the user to
   run a terminal command.
7. The final 0.3.5 installed plugin passes the full workflow in both Codex
   Desktop and DSH.
8. Test fixtures and runtime residue are removed without altering the user's
   real ART knowledge or governance history.
