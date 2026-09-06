# Codex plugin launch repair acceptance

Date: 2026-09-06. Scope: local plugin patch on ART 0.3.0; no public release
or Rust binary replacement is claimed.

## Failure and repair

A Codex host started from a restricted SSH environment did not inherit the
user-local executable directory in PATH. The plugin's bare `art` command
therefore failed before MCP initialization, while the same executable worked
from an interactive shell. The plugin skill separately named a feedback
operation that the actual MCP server did not expose.

The plugin now runs a POSIX launcher from its own package directory. It keeps
PATH installations working, falls back to the standard user-local install
link, and uses `exec` so the host retains ownership of the ART process. The
skill uses `art_feedback`.

## Failure reproduction and host acceptance

The initial two launch tests failed before the repair: minimal-PATH launch
raised executable-not-found, and a PATH installation exposed the skill/tool
naming mismatch. After refreshing the running host's plugin/skill discovery
cache and MCP configuration, the existing Codex task exposed all six tools
with a connected runtime. Native health, recall, exact read, a sourced
correction to an existing private memory, and readback of that revision
succeeded. Native feedback also marked a historical routing Edition stale
without changing its immutable content. These were repair-stage operations;
the subsequent comprehensive acceptance used read-only formal access.

The first comprehensive fresh-session run found another instruction gap: both
sessions attempted zero result limits, and one exceeded the token budget.
ART correctly rejected those requests; the sessions corrected the parameters
and retrieved the requested records. The skill now documents the existing
128..6000 token bound, 1..20 result bounds, null/default behavior, and the fact
that zero does not disable a recall lane. These first runs are not counted as
error-free acceptance.

Two more ephemeral Codex 0.153.4 processes then ran the same acceptance prompt,
one with the ordinary home and one with the managed home. Each used the real
enabled plugin and restricted host PATH, with session databases redirected to
task-owned temporary directories. The prompt provided thematic queries, not
expected record IDs or answer text, and prohibited shell ART or database
fallbacks. Each session completed seven native MCP calls with zero failures:
health, a lexical topic route, two lexical recalls, and three exact reads.
Both found the historical decision, the latest corrected private revision,
and the same shared operational Knowledge Edition. Their exact-read result
hashes matched across sessions. Only reading the installed skill used a shell.
The existing desktop task also passed native health after these runs.

## Comprehensive verification

| Check | Result |
| --- | --- |
| Rust workspace, all features, locked dependencies | 119 passed, zero failed; one performance test separately executed below |
| Optimized performance contract, 10,000 private records and 5,000 shared Editions | Passed; startup 1 ms, cold recall 86 ms, capture p95 1 ms, recall p95 123 ms, concurrent maximum 29 ms |
| Launcher contracts against installed binary | Eight passed |
| Temporary macOS arm64 archive, extracted plugin | All plugin files byte-identical to source; the same eight launcher tests passed |
| MCP tool contract | Exactly six Agent-safe tools; skill tool names match |
| Connection stress on installed binary | 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in one process, eight concurrent clients; nine idle file descriptors; final doctor healthy |
| Install lifecycle | Clean install, same-version reinstall, uninstall preserving data, reinstall, explicit fixture purge passed |
| Migration fixture | Review requirement, exact receipt verification, idempotency, Chinese recall, and changed-source rejection passed |
| Encrypted backup and recovery | Passed against isolated homes and a local bare Git repository, including revoked/active Editions and control-state restoration |
| Formatting and static analysis | Formatting and all-workspace/all-target/all-feature Clippy with warnings denied passed |
| Supporting checks | Three retrieval-harness tests, version consistency, website, launch surface, source identity, license policy, and changed-file credential/path checks passed |
| Dependency security audit | Passed for 301 dependencies against 1,239 advisories, with warnings denied |
| Independent review | No blocking finding after test hardening and skill clarification |

Launcher coverage includes restricted PATH, home and custom-bin paths with
spaces, PATH precedence, argument preservation, missing or non-executable
installations, broken install links, missing HOME, missing Agent without
implicit creation, clean stderr, EOF shutdown, and reconnection. MCP response
reads enforce a total timeout and validate JSON-RPC version and request ID.

The security audit's default Git transport initially failed to fetch. The
successful retry used an official advisory archive pinned to upstream commit
`5a0ebedfe8bdd2e295b171f4162f8c977bcad9a5` dated 2026-09-02, downloaded to the
task's directory; audit ran with `--no-fetch`, without suppressing findings or
allowing stale data. Backup acceptance used a temporary official age v1.3.2
binary verified against its release asset SHA-256. No system-wide dependency
installation or network-setting change was required.

## Final state and limits

The ordinary and managed Codex configurations both retain disabled built-in
memory features, generation, and use, with ART enabled. The plugin manifest,
launcher, and skill match across the producer checkout, local plugin source,
and installed plugin cache. The installed ART 0.3.0 binary remains unchanged:
`52638400b6c147c25ea2a656734d3657c8e605e9ef5e7fb025c99986ec17f8e4`.

Formal state before and after comprehensive verification remained at four
private memories, five revisions, eleven anchors, 34 proposals, and 28 current
shared Editions. Integrity, projection hashes, search indexes, and navigation
indexes passed; no pending recovery remained. Synthetic write, migration,
publication, revocation, backup, and load fixtures used disposable homes.

This is local macOS launch-repair acceptance, not a cross-platform or public
release approval. The new sessions were real ephemeral Codex CLI processes
using both home configurations, not newly created desktop sidebar tasks.
No external embedding provider is configured: native acceptance proved the
default lexical path, and the Rust suite covered semantic fallback contracts.
Linux, DSH, external-provider integration, and public release publication were
outside this repair's local acceptance scope. No commit, tag, push, or public
release was performed.

## Cleanup

After process-ownership inspection found no open task files, the temporary
homes, test databases, local backup repositories, downloaded test tools and
advisory data, compiled targets, package candidate, and diagnostic logs were
removed (2,833,464 KiB allocated, approximately 2.70 GiB).
Two empty parent directories created by existing test scripts were also
removed. This deletes disposable test artifacts permanently; it is not a
recoverable deletion of user data. The source patch, this report, installed
runtime, formal memory/knowledge, user history, and shared caches remain.

Available space was 91.34 GiB before testing,
88.71 GiB immediately before cleanup, and
91.30 GiB afterward. These are whole-volume observations and
can also reflect concurrent activity. No extra worktree was created. The
existing `codex/art-mcp-startup` branch retains the uncommitted repair and
new tests/report; the checkout is deliberately not reported as clean.
