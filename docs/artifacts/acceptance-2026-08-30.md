# ART 0.1.0-rc.1 acceptance evidence

Date: 2026-08-30, Asia/Shanghai. Candidate branch: `codex/art-v0.1`. No tag, push, installation, or AAA integration was performed.

## Build and automated gates

- Target: Apple M4, arm64, Darwin 25.6.0.
- Toolchain: Rust 1.98.0, Edition 2024; release build used `--locked`.
- `tests/scripts/release-gate.sh`: PASS.
- Workspace: 69 non-ignored tests passed; the separate release performance contract passed.
- Security: RustSec scanned 194 locked dependencies with no warning; license policy, secret scan, path/symlink/hard-link tests, debug-output privacy, source independence, and terminology scan passed.
- Recovery: six publication crash boundaries, lifecycle-event recovery, disk-full, read-only, migration race, WAL, manifest corruption, and private/shared search-index corruption passed.
- Stress: 500 graceful sessions in 47.5s; 100 abnormal disconnects; 1,000 queries in 0.757s; 8 concurrent clients; idle FD 9; final Doctor `ok`.
- Performance dataset: 10,000 private MemoryArtifacts and 5,000 Knowledge Editions. Startup 1ms; cold recall 86ms; capture p95 0ms; recall p50/p95/p99 1/1/1ms; concurrent maximum 11ms.
- Chinese recall: 64 Chinese and mixed technical golden queries passed, including exact IDs, paths, versions, errors, and cross-Agent negatives without embeddings.

## Real local host E2E

Host versions: Codex CLI 0.151.0 and DSH 0.1.1-rc.2. All runs used `/tmp/art-final-e2e-20260830`, temporary command-line/overlay configuration, and new host sessions.

- Codex primary captured a synthetic private artifact; a new Codex session recalled it. Codex secondary read of the exact reference returned `ART_NOT_FOUND`.
- DSH primary captured a synthetic private artifact; a new DSH session recalled it. DSH secondary read of the exact reference returned `ART_NOT_FOUND`.
- Human CLI composed two exact private revisions into a proposal, approved it, and published one Edition.
- Codex secondary and DSH secondary each recalled the same committed Edition. After human revocation, both new recalls returned zero Knowledge Editions.
- DSH transport recovery was exercised by terminating the task-owned ART child. DSH created a replacement child and completed 10/10 health calls; final Agent/Knowledge integrity and pending recoveries were healthy. ART guarantees bounded shutdown and integrity; reconnect policy remains host-owned.
- After every host command, no task-owned ART, Codex, or DSH process remained.

## Non-destructive boundaries

- A private Markdown knowledge tree dry-run found 28 Markdown files and created only import-proposal projections. Source content, file metadata, Git status, HEAD, and index tree were unchanged. The corrected source-only proof passed.
- DSH settings and headless patch hashes remained unchanged.
- Codex CLI added task-directory trust entries to its formal config even with `--ignore-user-config`. The three exact ART task entries were removed during cleanup and no ART path or server entry remains. The whole-file hash cannot be asserted equal to the initial snapshot because unrelated candidate-workspace entries changed concurrently; ART-specific residue is zero.
- No existing Across repository, plugin runtime, Basic Memory runtime, formal Agent MCP configuration, or AAA source file was changed by ART.

## Cleanup and retained artifacts

- Task-owned temporary Vaults and DSH session evidence (12MiB) were moved to recoverable Trash.
- Cargo removed 54,673 debug-profile files (reported 8.0GiB). Free disk increased from 93GiB to 99GiB.
- The 691MiB release profile, shared Cargo caches, RustSec advisory cache, source repository, machine-readable evidence, and final binary were intentionally retained. No task process remained.

## Known limits

ART 0.1 does not claim protection from an arbitrary process running as the same OS user with unrestricted filesystem access. It has no daemon, HTTP API, embedding runtime, cloud replication, automatic knowledge approval, automatic physical deletion, AAA adapter, or production cross-Agent broker. The optional cross-Agent files are contracts only.
