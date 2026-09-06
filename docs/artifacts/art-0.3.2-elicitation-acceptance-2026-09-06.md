# ART 0.3.2 MCP Elicitation Acceptance

**Date:** 2026-09-06  
**Candidate commit:** `adcff9541a81b407a095ec6d4bd11a22cfa01d76`  
**Branch:** `codex/art-mcp-elicitation-0.3.2`  
**Result:** Accepted for operator review; not tagged, published, merged, or installed.

## Accepted boundary

ART exposes exactly seven Agent-safe MCP tools. The new
`art_knowledge_governance` input contains only `operation`, `proposal_id`, and
`revision`. Human review decision, reason, actor, and publication confirmation
are collected only through MCP form Elicitation. Review and publication use
separate requests. Unsupported, declined, cancelled, malformed, stale, and
replayed flows fail closed without inventing consent.

The existing Knowledge Vault remains the sole writer of review receipts and
immutable Editions. Elevated or high-risk single-source proposals still
require a distinct second human operator.

## Protocol evidence

The Tokio duplex rmcp client/server suite contains ten governance tests. It
proved:

- approve, request changes, and reject review decisions;
- ART-generated bounded reviewer identity;
- separate review and publication Elicitations;
- publication only after `accept + confirm=true`;
- decline, cancel, false confirmation, blank/oversized reason, and malformed
  response no-write behavior;
- unsupported-client CLI fallback;
- wrong-revision and publication replay protection;
- snapshot drift after human confirmation blocks publication;
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

A second run captured one disposable memory, proposed one exact revision, and
called `art_knowledge_governance(operation=review)`. The first attempt exposed
the incompatible root `title`; commit `541e908` replaced typed schema generation
with an explicit MCP-compatible form schema. The repeated real Codex call
accepted the schema with no parse error. Because `codex exec` is noninteractive,
Codex returned cancellation; ART returned `USER_CANCELLED`, kept the Proposal
`submitted`, and the Agent supplied no decision. Positive accept/publish UI
semantics were therefore proven by the interactive protocol harness, not by
pretending a noninteractive Codex process was a human.

The Codex model transport timed out before both successful runs and recovered
by falling back from WebSocket to HTTPS. This did not affect MCP connection or
ART state.

## Automated and release gates

- `cargo fmt --all -- --check`: passed.
- workspace Clippy with all targets/features and warnings denied: passed.
- workspace tests with all targets/features: 129 passed, with one ordinary-layer
  release performance test ignored by design and executed by the release gate.
- release performance acceptance: passed with 10,000 private memories and
  5,000 shared Editions.
- release gate: passed, including migration, install lifecycle, plugin launch,
  exact seven-tool surface, site/launch checks, independence scan, dependency
  audit, secret scan, and stress acceptance.
- stress acceptance: 500 graceful sessions, 100 abnormal disconnects, 1,000
  queries in one process, eight concurrent clients, idle FD count 9, final
  Doctor status `ok`.

## Candidate package

The native macOS arm64 archive was built through
`scripts/build_release_binary.sh` so Rust source paths were remapped, then
packaged deterministically. It contained nine sorted, timestamp-normalized
members; the plugin and provenance versions were `0.3.2`; the provenance commit
matched the candidate; and the binary contained no `/Users/fanhcy/` path.

- Archive SHA-256:
  `b59fc465af5abf9feae1551fd0fb50b6992af7f7bc086329d14f5b1fff9b3f68`
- Binary SHA-256:
  `7d8f4afb83163523388f1338f7c5a572bd7f2ac16e5f67542310b1056f04ac6d`

The cross-platform aggregate `.mcpb` was not fabricated locally because a
Linux amd64 release binary was not available on this Mac. The release workflow
builds both native targets before aggregate verification. Its manifest and
verifier now require all seven tools, including `art_knowledge_governance`.

## Isolation and formal-state check

All mutating acceptance used a task-owned `/tmp` ART home. It ended with one
memory, one submitted Proposal, zero Editions, and zero pending publication
intents. The formal ART installation remained version `0.3.1`, healthy, and
unchanged at six memories, nine revisions, 22 anchors, 35 Proposals, and 28
current Editions. No formal plugin, Codex configuration, ART home, review, or
Knowledge Edition was modified.

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
