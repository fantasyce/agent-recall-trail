# ART 0.3.3 Delegated Governance Acceptance

**Status:** Candidate accepted locally; not published

**Branch:** `codex/art-delegated-governance-0.3.3`

**Date:** 2026-09-07

## Candidate contract

ART 0.3.3 adds an ART-owned local governance page for Codex Desktop and DSH,
persistent default-off delegation scoped to one host binding plus Agent, one
atomic `approve_and_publish` operation, and linked `AgentDelegated` receipts.
Native MCP form Elicitation remains compatible for clients that support it.

The candidate has eight Agent-safe MCP tools. Agent input to delegated
governance remains limited to operation, Proposal ID, and exact revision.
Human-facing settings, review, publication, status, and audit use the local
page rather than a terminal workflow.

## Required evidence

- exact candidate commit and version consistency;
- formatting, warnings-as-errors analysis, workspace tests, and release gate;
- isolated installation and MCP tool discovery;
- random `127.0.0.1` page binding, capability, Origin, CSRF, and revision checks;
- delegation enable, restart persistence, atomic publication, distinct audit,
  immediate disable, and default-off behavior;
- Codex Desktop in-app browser journey;
- DSH page journey or an explicit environment limitation;
- residue, disk use, and final Git state.

## Accepted candidate

- Tested source commit: `06fcb2e7ce9dcef24a99ece6f58352a425050edb`.
- Candidate binary: `target/release/art`, version `0.3.3`, 19,999,344 bytes.
- SHA-256: `04fe1d0a09c3ccd0f0fb708e65e8e5b07df3a930d8fb8d40490f1279f4e15ef3`.
- Publication state: local branch only. No tag, remote push, marketplace
  update, or installed-user ART replacement was performed.

## Automated verification

`bash tests/scripts/release-gate.sh` completed successfully against the tested
commit. It included:

- formatting and warnings-as-errors checks;
- all workspace unit, integration, MCP stdio, document, and release tests;
- the non-ignored 10k-memory / 5k-Edition release performance contract;
- exact eight-tool MCP discovery;
- isolated install, repair, uninstall, and launch-surface contracts;
- site checks, independent-product-expression scan, and
  `OPEN_SOURCE_CHECK=PASS`;
- 500 graceful sessions, 100 abnormal disconnects, 1,000 queries through one
  process, eight concurrent clients, idle FD count 9, and healthy doctor;
- RustSec advisory scan of 308 locked dependencies.

The unsupported-Elicitation contract now returns
`next_action=open_governance_ui` and no terminal fallback field. Native form
Elicitation remains available to a client that actually supports and renders
it.

## Codex Desktop acceptance

The release binary ran against an isolated ART home and the real
`art_governance_ui_open` result was opened in Codex Desktop's in-app browser.
The capability-bearing query value is intentionally omitted from this record.

1. The Settings view identified `codex-primary` and the expected host binding.
2. Enabling delegation changed the persisted policy to `delegated_local`.
3. After process restart, `art_health` still reported `delegated_local`, proving
   the policy is configured once rather than per command.
4. A private memory produced Proposal
   `artp_01M1VWMMK6Y4G1G7NX7X1VTBQ9@1`.
5. One `approve_and_publish` call returned `outcome=published`,
   `proposal_status=materialized`, `actor_type=agent_delegated`, and Edition
   `arke_01M1VWMTEZ8FD9DY4KZT24ZQSY`.
6. The visible Audit view showed separate approval and publication records,
   both labeled `agent_delegated` and bound to the same Agent and host.
7. The page toggle was switched off. After another process restart,
   `art_health` reported `governance_mode=human_review`, with the Agent Vault,
   Knowledge Index, and navigation maps healthy and no pending recovery.

The page listened only on a random `127.0.0.1` port. Automated UI contracts
also covered expiring high-entropy sessions, Origin and CSRF enforcement,
proposal/revision scoping, server reuse, and real policy mutation. A visual
inspection confirmed the Chinese Settings and Audit views rendered without
clipping or external assets; keyboard focus and reduced-motion behavior are
covered by the source contract.

## DSH acceptance

Installed `dsh 0.1.1-rc.2` composed the temporary ART overlay with
`serverName=art`, stdio transport, the release binary, isolated ART home, and
`dsh-primary`. `failOnStartupError=true` remained active. DSH Web started on a
random loopback port, its browser UI rendered in Codex, its MCP client plugin
was mounted and enabled, and the ART stdio child was bound to `dsh-primary`.
Stopping DSH stopped that child as expected.

No DSH model prompt was sent: doing so would have created a record in the
operator's existing DSH workspace and made an external model request. The
shared ART-owned governance page itself was fully exercised through Codex; DSH
uses the same URL and static application returned by the same MCP tool.

## Residue and boundaries

- All ART MCP and task-owned DSH processes were shut down; no attributed
  listener remained.
- Two isolated acceptance homes remain for review because Agent Residue
  Evidence does not authorize automatic cleanup:
  `/tmp/art-0.3.3-browser-acceptance.XdlH3l` (300 KiB) and
  `/tmp/art-0.3.3-dsh-acceptance.4rCf0c` (292 KiB).
- An older separately owned browser prototype at port 64091 was not changed or
  stopped.
- The tested commit was clean before this acceptance record was appended.

## Decision

The local 0.3.3 candidate satisfies the approved design: governance is
page-first for people, delegation is visibly marked and default-off, one
explicit current instruction can atomically approve and publish when the
persistent policy is enabled, and disabling the policy restores human review.
The branch is ready for human source review and a separate release decision.
