# ART 0.3.2 MCP Elicitation Acceptance

**Date:** 2026-09-06
**Candidate source commit:** `e56acb0f1a6047c504b10859dd7761c078b5aa48`
**Branch:** `codex/art-mcp-elicitation-0.3.2`
**Result:** Accepted for operator review; not tagged, published, merged, or installed.

## Accepted boundary

ART exposes exactly seven Agent-safe MCP tools. The new
`art_knowledge_governance` input contains only `operation`, `proposal_id`, and
`revision`. Human review decision, reason, actor, and publication confirmation
are collected only through MCP form Elicitation. Review and publication use
separate requests. Unsupported, declined, cancelled, malformed, stale, and
replayed, timed-out, source-invalidated, and concurrent flows fail closed
without inventing consent.

The existing Knowledge Vault remains the sole writer of review receipts and
immutable Editions. Elevated or high-risk single-source proposals still
require a distinct second human operator.

## Protocol evidence

The Tokio duplex rmcp client/server suite contains sixteen governance tests. It
proved:

- approve, request changes, and reject review decisions;
- ART-generated bounded reviewer identity;
- separate review and publication Elicitations;
- publication only after `accept + confirm=true`;
- decline, cancel, false confirmation, blank/oversized reason, and malformed
  response no-write behavior;
- missing and empty Elicitation capabilities both produce a CLI fallback;
- wrong-revision, terminal-state re-review, and publication replay protection;
- real private-memory revision drift and source invalidation block governance;
- a bounded timeout performs no write and a later request can retry;
- malformed review and publication payloads perform no write and can retry;
- concurrent reviews commit one receipt, and concurrent publication
  confirmations materialize one Edition;
- Edition numbers are reserved atomically; a competing same-key proposal must
  be presented again with the new number before publication;
- elevated single-source content stops at `under_review` for a second human.

The Elicitation schema root is restricted to `$schema`, `type`, `properties`,
and `required`. This regression was added after real Codex rejected rmcp's
automatically generated root-level `title` field.

## Local Codex acceptance

The installed client was `codex-cli 0.153.4`. Its feature inventory reported
both `auth_elicitation` and `tool_call_mcp_elicitation` as stable and enabled.
An ephemeral, isolated Codex run connected directly to the candidate binary
and successfully called `art_health`, receiving `binary_version=0.3.2` and
`bound_agent_id=codex-primary`.

A pre-review run captured one disposable memory, proposed one exact revision,
and called `art_knowledge_governance(operation=review)`. The first attempt exposed
the incompatible root `title`; commit `541e908` replaced typed schema generation
with an explicit MCP-compatible form schema. The repeated real Codex call
accepted the schema with no parse error. Because `codex exec` is noninteractive,
Codex returned cancellation; ART returned `USER_CANCELLED`, kept the Proposal
`submitted`, and the Agent supplied no decision. Positive accept/publish UI
semantics were therefore proven by the interactive protocol harness, not by
pretending a noninteractive Codex process was a human.

After the concurrency fixes, the final remapped candidate was tested again
with `codex-cli 0.153.4`. Codex called `art_health` and received
`binary_version=0.3.2` and `bound_agent_id=codex-primary`. It then submitted a
disposable Proposal and invoked the governance tool with the exact three-field
input. Codex accepted the form schema and returned `USER_DECLINED`; ART kept
the Proposal `submitted`, wrote no review receipt, created no Edition, and left
no pending publish intent.

The Codex model transport timed out before both successful runs and recovered
by falling back from WebSocket to HTTPS. This did not affect MCP connection or
ART state.

## Automated and release gates

- `cargo fmt --all -- --check`: passed.
- workspace Clippy with all targets/features and warnings denied: passed.
- workspace tests with all targets/features: 140 passed, with one ordinary-layer
  release performance test ignored by design and executed by the release gate.
- release performance acceptance: passed with 10,000 private memories and
  5,000 shared Editions.
- release gate: passed, including migration, install lifecycle, plugin launch,
  exact seven-tool surface, site/launch checks, independence scan, dependency
  audit, secret scan, and stress acceptance.
- stress acceptance: 500 graceful sessions, 100 abnormal disconnects, 1,000
  queries in one process, eight concurrent clients, idle FD count 9, final
  Doctor status `ok`.
- focused governance evidence: 24 Knowledge Vault contracts and 16 rmcp
  Elicitation contracts passed, including atomic compare-and-swap, terminal
  state, source drift, malformed response, timeout, and concurrency cases.

## Candidate package

The native macOS arm64 archive was built through
`scripts/build_release_binary.sh` so Rust source paths were remapped, then
packaged deterministically. It contained nine sorted, timestamp-normalized
members; the plugin and provenance versions were `0.3.2`; the provenance commit
matched the candidate; and the binary contained no developer-home path.

- Archive SHA-256:
  `36d0e8b1bfc0387b6a4207a151141f706bd3652b9a1636bddb2d6693fba3234e`
- Binary SHA-256:
  `3c60517ff5e2d08abb378ab0080891b9add0b7195e93e430a3dbccb96c734e51`

The cross-platform aggregate `.mcpb` was not fabricated locally because a
Linux amd64 release binary was not available on this Mac. The release workflow
builds both native targets before aggregate verification. Its manifest and
verifier now require all seven tools, including `art_knowledge_governance`.

## Isolation and formal-state check

All mutating acceptance used a task-owned temporary ART home. It ended with one
memory, one submitted Proposal, zero Editions, and zero pending publication
intents. The formal ART installation remained version `0.3.1`, healthy, and
unchanged at six memories, nine revisions, 22 anchors, 35 Proposals, and 28
current Editions. No formal plugin, Codex configuration, ART home, review, or
Knowledge Edition was modified.

The first independent code review rejected the earlier candidate because
terminal Proposals could be reviewed again and governance writes did not bind
snapshot validation atomically. Source commit `a825e98` addressed every
reported critical and important issue: state-machine enforcement, immediate
transactions, publication reservations, explicit form capability detection,
source revalidation, complete publication context, bounded stable outcomes,
timeout/malformed/concurrent tests, public copy, and host-bound reviewer
identity. Follow-up review then found two recovery interactions introduced by
the reservation mechanism. Commits `08f25df`, `8cd8e28`, and `e56acb0` make
partial recovery release its number, make complete recovery materialize the
Proposal, and scope active-process cleanup to only the failed intent so it
cannot disturb another concurrent publication. The independent final review
reported no remaining blockers. The final full gate above was run against
`e56acb0` after all of those changes.

Agent Residue Evidence tools were not callable in this session, so observation
used exact task-owned paths, process checks, and before/after diagnostics. The
temporary ART home, candidate archive directory, and task-owned build output
are removed after final verification; the source branch, design, plan, and this
acceptance record are retained.

## Remaining operator action

Review the branch. A later release action must build the Linux artifact in CI,
verify the aggregate `.mcpb`, merge through the repository's normal process,
tag the exact `origin/main` commit, and publish explicitly. None of those
actions were performed here.
