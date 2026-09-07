# ART v0.3.3 Release Acceptance — 2026-09-07

## Release contract

- Source: `main`, followed by immutable tag `v0.3.3`; the tag must resolve to
  the exact pushed `origin/main` commit.
- Authorization: the repository release gate must pass before `main` is
  pushed. The tag-triggered protected workflow repeats tests and builds the
  release assets before publication.
- Channels: GitHub Release, MCP Registry, and the project site/source tree.
- Artifacts: macOS arm64 archive, Linux amd64 archive, MCP bundle, Registry
  metadata, SPDX SBOM, SHA-256 manifest, and GitHub provenance attestations.
- Consumers: the public macOS archive must install the real CLI/MCP runtime;
  Codex Desktop and DSH must load the released plugin/integration and complete
  representative health, recall, exact-read, governance, and shutdown checks.
- Cleanup: after every public channel and consumer passes, remove task-owned
  build/download/acceptance state, remove all non-`main` local and remote
  branches, and retain only the released runtime, source `main`, tag, public
  evidence, and user data.

## Completion matrix

| State | Predicate | Status |
|---|---|---|
| contract | Repository rules, channels, consumers, rollback, and cleanup identified | complete |
| candidate | Exact source is internally consistent and final assets verify | in progress |
| authorized | Full release gate passes on the source to be pushed | pending |
| source-bound | `origin/main`, local `main`, and `v0.3.3` resolve identically | pending |
| published | GitHub Release and MCP Registry accept 0.3.3 | pending |
| publicly-verified | Public assets, digests, metadata, provenance, and site read back | pending |
| accepted | Public artifact works in CLI, Codex Desktop, and DSH | pending |
| recorded | This report and ART release memory bind final evidence | pending |
| cleaned | Authorized temporary state and non-main branches are absent | pending |

## Timing record

| State | Started | Ended | Kind | Input | Receipt / reason |
|---|---|---|---|---|---|
| contract | 2026-09-07T14:01:21+08:00 | 2026-09-07T14:02:05+08:00 | active | `84092d9` | Repository contract rebuilt; final asset verifier found stale seven-tool assertion |
| candidate-rework-1 | 2026-09-07T14:02:05+08:00 | 2026-09-07T14:02:08+08:00 | rework | `84092d9` | First local aggregate used the ordinary test binary and correctly failed the private-build-path scan |
| candidate-rework-2 | 2026-09-07T14:02:32+08:00 | 2026-09-07T14:03:18+08:00 | rework | `84092d9` | Official remapped build passed path scanning and exposed the stale seven-tool MCP bundle assertion |
| candidate-fix | 2026-09-07T14:03:18+08:00 | 2026-09-07T14:05:18+08:00 | active | pending commit | Verifier now requires the exact eight-tool set; the full release gate now builds and verifies aggregate release assets |
| authorization-1 | 2026-09-07T14:06:14+08:00 | 2026-09-07T14:07:05+08:00 | rework | `bd7cd80` | All earlier checks passed; independent-product-expression scan rejected one generic release-process term in this report |

## Release evidence

Evidence is appended as the release advances. Failed attempts remain recorded
instead of being overwritten.

The focused post-fix aggregate verification passed for both native archive
slots, the MCP bundle, Registry metadata, SBOM, checksums, deterministic member
layout, provenance binding, private build-path scan, and exact eight-tool MCP
surface. This focused receipt does not authorize publication; the full gate
must pass again after the fix is committed.
