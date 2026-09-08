# ART 0.3.4 Anchor Kind Contract Acceptance

**Date:** 2026-09-07

**Source:** the commit containing this acceptance record on local `main`

**Publication status:** local candidate only; no tag, push, registry update, or
formal host replacement was performed.

## Decision

The local ART 0.3.4 candidate satisfies the approved anchor-kind contract.
`art_memory_capture.anchors[].kind` is now the domain `AnchorKind` enum, the MCP
schema exposes exactly eight snake_case values, invalid aliases are rejected at
input decoding with canonical alternatives, and every shipped skill documents
the same vocabulary.

The strict review also found that the checked-in MCP schema had remained at a
six-tool snapshot after 0.3.3 added two governance tools. The artifact is now an
exact runtime-generated eight-tool snapshot, and the release gate compares it
byte-for-byte on every run.

## TDD evidence

- The schema test failed because `kind` was an unconstrained string.
- The invalid-input test failed because `kind: "git"` deserialized successfully.
- The packaged-skill test failed on the first missing canonical value.
- The generated-artifact test failed at the first stale tool-schema byte.
- After the minimal changes, all four contracts passed.
- Replacing the typed field with the former string form caused the regression
  target to fail to compile against enum fixtures; restoring the typed field
  returned both focused anchor tests to green.

## Automated acceptance

- `cargo fmt --all --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `cargo test --workspace --all-features -- --test-threads=1`: pass, including
  14 MCP, 18 Elicitation, 28 knowledge, 19 recall, 18 private-vault, 14 CLI,
  8 backup, 6 stdio, 6 embedding, 4 semantic-projection, 3 context, 11 domain,
  and 2 governance-page contract tests.
- `bash tests/scripts/release-gate.sh`: pass. This additionally ran the ignored
  10k-private/5k-shared release performance contract, 500 graceful MCP
  sessions, 100 abnormal disconnects, 1000 one-process queries, eight concurrent
  clients, migration, install/uninstall lifecycle, dual-platform release assets,
  MCPB, SBOM, dependency advisory scan, license policy, public-language,
  independence, secret, site, and launch gates.
- All three shipped ART skills passed the Codex skill validator in an isolated
  temporary Python environment.

## Protocol and host evidence

- The packaged stdio MCP launch test inspected `tools/list`, resolved the
  `AnchorKind` reference, captured and replayed one memory with both
  `git_object` and `external_document`, rejected `git` and `url` with canonical
  alternatives, and recalled no records for the rejected requests. Eight launch
  and reconnect variants passed.
- A real ephemeral Codex 0.153.4 task loaded only the candidate MCP, observed
  `binary_version=0.3.4` and `bound_agent_id=codex-primary`, captured a
  `git_object`-anchored memory, and read revision 1 back from the isolated home.
  The first custom-MCP attempt was host-blocked before ART by the `never`
  approval policy; the accepted run used Codex's task-scoped automatic review.
- DSH 0.1.1-rc.2 headless overrode its already-installed `art-memory` entry,
  observed `0.3.4 / dsh-primary`, and captured/read an
  `external_document`-anchored memory.
- DSH Web used the repository's insert overlay because that profile did not yet
  contain `art-memory`. Its visible page showed successful `art_health` and
  `art_governance_ui_open` calls. The returned ART page visibly showed
  `dsh-primary`, loopback-only binding, and delegation still disabled. No
  governance setting was changed.

## Evidence ladder

| Layer | Evidence | Result |
| --- | --- | --- |
| source | one enum type; duplicate MCP parser removed | pass |
| generated schema | runtime output equals checked-in eight-tool JSON byte-for-byte | pass |
| package | Codex plugin, MCPB, archives, registry metadata, SBOM | pass |
| isolated install | install, repair, uninstall, reinstall, purge | pass |
| live service | real stdio initialize/list/call/read and bounded shutdown | pass |
| Codex host | candidate health, capture, exact read | pass |
| DSH host | headless capture/read and visible Web governance page | pass |

## Hygiene and limits

The public 0.3.3 binary, Codex plugin, DSH configuration, ART data, and live
formal MCP processes were not replaced. Three DSH test sessions were removed
from active storage and their exact cache entries were removed atomically; the
session logs were moved to Trash for recovery rather than irreversibly deleted.
Task-owned servers were stopped and their listening ports closed.

The Agent Residue Evidence skill was available, but its required MCP observation
tools were not exposed in this task. Residue evidence therefore uses explicit
before/after filesystem, process, port, Git, and disk checks rather than an ARE
report. This is an evidence-format limitation, not skipped product coverage.
