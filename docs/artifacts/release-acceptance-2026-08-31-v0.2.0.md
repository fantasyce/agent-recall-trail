# ART v0.2.0 release acceptance — 2026-08-31

## Decision

ART v0.2.0 passes the complete source, packaging, lifecycle, stress,
performance, security, dependency, and public retrieval-quality gates. The
release preserves the six-tool MCP boundary, per-Agent private Vaults,
human-reviewed Knowledge Editions, encrypted Knowledge Vault recovery, and a
fully functional lexical fallback with vector status explicitly unavailable.

## Retrieval quality

The compiled v0.2.0 `art recall` product path evaluated the complete public
BEIR SciFact and NFCorpus test splits with a result depth of ten. Generated
Vaults were temporary and removed automatically.

| Dataset | Documents | Queries | Recall@10 | nDCG@10 | nDCG@3 | CLI p95 | Gate |
|---|---:|---:|---:|---:|---:|---:|---|
| SciFact | 5,183 | 300 | 0.798944 | 0.669225 | 0.621441 | 348.623 ms | PASS |
| NFCorpus | 3,633 | 323 | 0.149301 | 0.309516 | 0.363571 | 337.874 ms | PASS |

The latency includes process startup, configuration, Vault open, retrieval,
serialization, and shutdown. Dataset archive commitments remain:

- SciFact MD5: `5f7d1de60b170fc8027bb7898e2efca1`
- NFCorpus MD5: `a89dba18a62ef92f7d323ec890a0d38d`

## Release gates

- Formatting, all-features warnings-as-errors analysis, workspace tests,
  migration, release-size performance, plugin surface, and temporary
  install/repair/uninstall lifecycle: PASS.
- Stress: 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in
  one process, 8 concurrent clients, idle file-descriptor count 9, and final
  Doctor healthy: PASS.
- Open-source independence, launch surfaces, secret/private-path scan,
  RustSec audit of 194 locked dependencies, and license policy: PASS.
- No benchmark corpus, query, qrel, generated Vault, formal ART data, recovery
  authority, or machine-private knowledge is included in the repository.

## Publication boundary

This document qualifies the source candidate for protected-main publication.
Public native artifacts, exact tag-to-main identity, GitHub provenance, MCP
Registry publication, and installed Codex/DSH E2E are verified separately
after the tag workflow completes.
