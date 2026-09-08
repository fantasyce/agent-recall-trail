# ART 0.3.5 Governance Proposal Detail Acceptance

**Date:** 2026-09-09

**Tested source commit:** `c190c96c2bef01070a0a55f0a9e3b9650c7359fc`

**Branch:** `codex/art-governance-proposal-detail-0.3.5`

**Publication status:** local candidate accepted; no tag, push, registry
publication, merge, or formal user installation was performed.

## Decision

The ART 0.3.5 candidate satisfies the approved Knowledge Proposal detail and
human-review design. The summary queue exposes no decision shortcut. Exact
proposal detail contains the complete canonical content, sanitized rendering,
raw Markdown, locked source metadata, hashes, ordered review history, verified
current-Edition comparison, governance requirements, and exact allowed actions.
Review and publication remain revision-, status-, draft-hash-, and
source-set-hash-bound and return authoritative receipts.

## Test-driven evidence

- Bootstrap-secrecy, exact authorization, sanitizer, comparison, receipt,
  snapshot-conflict, rejection-terminality, session-lifetime, view-permission,
  and review-history tests were added before or with their minimal production
  changes.
- The version contract was changed to 0.3.5 and observed failing against the
  0.3.4 candidate before the active release surfaces were updated.
- A release-gate run correctly failed on a developer-machine absolute path in
  the implementation plan. The path was replaced by the repository-relative
  design reference and the complete gate was rerun from the start.

## Automated acceptance

`bash tests/scripts/release-gate.sh` passed on the tested commit. It included:

- formatting and all-workspace/all-target/all-feature Clippy with warnings
  denied;
- every unit, integration, MCP stdio, Elicitation, governance-page, recovery,
  migration, and document test;
- the optimized 10,000-private-memory / 5,000-Edition performance contract;
- generated-schema, version, plugin-surface, install/repair/uninstall, site,
  launch, public-language, independence, secret, and license checks;
- verified `0.3.5` MCPB, macOS arm64 archive, Linux amd64 archive, registry
  metadata, checksums, and SBOM;
- 500 graceful sessions, 100 abnormal disconnects, 1,000 one-process queries,
  eight concurrent clients, nine idle file descriptors, and a healthy final
  Doctor result;
- RustSec audit of 333 locked dependencies against 1,242 advisories.

Focused contracts prove that malicious Markdown cannot retain scripts,
JavaScript URLs, event handlers, raw active HTML, iframes, styles, remote
images, or the session capability. They also cover stale revisions, statuses,
draft/source hashes, invalid and expired sessions, concurrent decisions,
independent elevated-risk review, first Editions, verified current Editions,
and publication-number conflicts without an unauthorized write.

## Browser acceptance

The real Codex in-app browser exercised task-owned proposals through the final
UI state machine:

- desktop right drawer and 759-pixel full-screen layout;
- rendered knowledge, raw Markdown, a grouped current-Edition diff, locked
  provenance, and ordered review history;
- exact-link auto-open, tab behavior, focus restoration, Escape protection,
  visible status text, and post-decision/post-publication focus;
- approve, request changes, reject, and two-step publish journeys;
- two simultaneous exact sessions where the second decision won and the first
  entered a persistent conflict state, disabled every mutation, and preserved
  its unsubmitted reason for copying.

The five-minute warning threshold, 30-minute fixed lifetime, expired-session
removal, reconnect/expiry read-only reason behavior, narrow breakpoint, and
reduced-motion mode are deterministic automated contracts; acceptance did not
wait 25 wall-clock minutes merely to reach the warning threshold.

## Final installed-host acceptance

The release binary was installed into a task-owned ART Home with SHA-256
`f870dad5703b44bede38728a4cd9e306b7dad4b772db4093988da804ec9bf503`.
The installed byte hash exactly matched `target/release/art`.

### Codex Desktop

A real ephemeral `codex-cli 0.153.4` process loaded only the installed candidate,
reported `binary_version=0.3.5`, `bound_agent_id=codex-primary`, and
`governance_mode=human_review`, captured one anchored private memory, proposed
`artp_01M218Q5FVCAD4BZFYXSQD66QT@1`, and opened its exact page. Codex Desktop's
in-app browser inspected and approved that exact proposal, confirmed immutable
publication separately, and received Edition
`arke_01M218S29CVYZZCFPWJN6DA1C8` with both Markdown and manifest hashes.

### DSH

Installed `dsh 0.1.1-rc.2` first ran its headless profile with an exact,
task-owned override of the existing `art-memory` slot. It reported
`binary_version=0.3.5`, `bound_agent_id=dsh-primary`, captured an anchored
memory, proposed `artp_01M218X6DP37C6ZT1Q80239CKF@1`, and opened exact review.
DSH Web then loaded the same installed candidate through its insert overlay,
visibly called `art_health` and `art_governance_ui_open`, and kept the ART child
alive while the exact page completed approval and publication. The page returned
Edition `arke_01M21922E7KJYXJZA55NDEB3H6` and its full Markdown/manifest receipt.

Both hosts used the same task-owned Knowledge Vault. Neither host configuration
nor the formal ART installation was changed.

## Hygiene and remaining actions

All attributed ART, Codex acceptance, DSH Web, and browser-fixture processes
were stopped. Task-owned ART Homes, candidate install, overlays, proposals,
sessions, and browser fixtures were removed from active locations. The two
task-created DSH session records and all temporary ART roots were moved to the
macOS Trash for recovery; their exact DSH projection-cache entries were removed.
No attributed listener or active task path remained.

The branch retains only source, tests, release metadata, the approved design,
implementation plan, and this acceptance record. Human source review and the
repository's normal merge/release authorization remain separate actions.
