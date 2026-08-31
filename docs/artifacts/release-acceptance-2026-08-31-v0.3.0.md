# ART v0.3.0 release acceptance — 2026-08-31

## Decision

**PASS.** ART v0.3.0 is accepted for public release. The source candidate,
native macOS candidate, installed runtime, real Codex host, and real DSH host
passed their mandatory gates. The accepted retrieval implementation is commit
`320744252bef83559cd6bf6377d49fbcf59dc89c`; release metadata and this report
may advance the tag without changing runtime code.

The release keeps lexical retrieval as the zero-configuration default, adds a
governed canonical full scan, and exposes provider-neutral semantic and hybrid
modes only when a user explicitly configures an embedding endpoint. Provider
quality is outside the ART qualification claim. Missing, failed, mismatched,
or stale optional projections fall back visibly to lexical retrieval.

## Retrieval quality

The compiled `art 0.3.0` lexical product path evaluated the complete public
BEIR SciFact and NFCorpus test splits at a result depth of ten. CLI latency
includes startup, configuration, Vault open, retrieval, serialization, and
shutdown.

| Dataset | Documents | Queries | Recall@10 | nDCG@10 | nDCG@3 | CLI p95 | Gate |
|---|---:|---:|---:|---:|---:|---:|---|
| SciFact | 5,183 | 300 | 0.798944 | 0.669225 | 0.621441 | 312.187 ms | PASS |
| NFCorpus | 3,633 | 323 | 0.149301 | 0.309516 | 0.363571 | 302.985 ms | PASS |

Both executions remained in lexical mode with vector status `unavailable`;
there was no hidden semantic fallback or provider-specific qualification.

## Architecture and governance

- All bounded retrieval modes admit identity, lifecycle, validity, current
  Edition, and revocation state before truncating their candidate window.
- Only currently Active and validity-eligible private memory may enter a
  user-configured embedding provider. Candidate, future-valid, expired,
  disputed, superseded, and archived memory remains local and unembedded.
- Crossing a validity boundary invalidates the private semantic epoch and
  causes a safe lexical fallback until an explicit reindex.
- Explicit Candidate supplements stay local, retain lower authority, and
  cannot displace or outrank Active semantic memory.
- An independent final code review found no remaining P0, P1, or P2 release
  blocker.

## Release gates

- Rust formatting, warnings-as-errors analysis, all-feature workspace tests,
  Python retrieval-harness tests, migration, native release performance,
  plugin surface, install/repair/uninstall lifecycle, site, launch surface,
  open-source independence, and secret scans: PASS.
- Stress: 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in
  one process, 8 concurrent clients, 9 idle file descriptors, and final Doctor
  healthy: PASS.
- RustSec audit scanned 301 locked dependencies against 1,233 advisories with
  no release-blocking vulnerability: PASS.
- Toolchain: `rustc 1.98.0 (88d9e12ae 2026-08-18)`.
- Path-neutral macOS arm64 binary SHA-256:
  `85f01e75939b29a7cf318735521ed554d8c0d04bb9d8041c3dc6ee1fa47678d2`.
- Isolated macOS candidate archive SHA-256:
  `e40d394cbf99ed20639f6af6dae5463d0759ab2f7d9939214fce3701defc776a`.
  Public workflow archives are rebuilt natively and separately attested.

## Installed real-host acceptance

Synthetic Agent identities `codex-v030-e2e` and `dsh-v030-e2e` used the exact
candidate binary through stdio MCP. Both hosts passed six-tool discovery,
health, route, lexical recall, governed full scan, exact read, shared Knowledge
Edition recall, cross-Agent private-memory denial, and unconfigured
semantic/hybrid lexical fallback.

The real locally installed Codex CLI invoked `art_health`, `art_recall`, and
`art_read` through the candidate MCP server and returned its exact PASS token.
The real DSH headless profile did the same. DSH was then killed after its ART
child had started; the child exited without residue, and a fresh DSH process
reconnected and recalled shared knowledge successfully.

## Public-data boundary

No formal private memory, Knowledge Edition, vector projection, endpoint
configuration, credential, private CA, raw host transcript, benchmark corpus,
query, qrel, or generated test Vault is committed or packaged. The repository
retains aggregate metrics and public contracts only. Disposable vectors are
excluded from knowledge Git and encrypted backups.

The optional embedding interface is release-qualified for configuration,
provider binding, resumable indexing, governance, staleness, and fallback—not
for any universal model recall percentage. No provider-specific semantic score
is used as an ART v0.3.0 release claim.
