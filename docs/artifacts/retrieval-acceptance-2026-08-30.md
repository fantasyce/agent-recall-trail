# ART retrieval acceptance — 2026-08-30

## Decision

The retrieval-quality candidate passes its source, public benchmark,
performance, security, packaging, lifecycle, and stress gates. This evidence
qualifies the candidate for release review; it does not claim that the formal
Codex or DSH installation has been replaced.

## Public benchmark

The checked-in product-path harness evaluated the complete BEIR test splits
through the compiled `art recall` command with a result depth of ten. Generated
Vaults were temporary and removed automatically. Only aggregate metrics are
recorded here.

| Dataset | Documents | Queries | Recall@10 | nDCG@10 | nDCG@3 | CLI p95 | Gate |
|---|---:|---:|---:|---:|---:|---:|---|
| SciFact | 5,183 | 300 | 0.798944 | 0.669225 | 0.621441 | 330.268 ms | PASS |
| NFCorpus | 3,633 | 323 | 0.149301 | 0.309516 | 0.363571 | 321.886 ms | PASS |

The CLI latency includes process startup, configuration load, Vault open,
retrieval, serialization, and shutdown. Both datasets ran with vector retrieval
unavailable, so the recorded quality is the deterministic lexical path.

Fixture archive commitments:

- SciFact MD5: `5f7d1de60b170fc8027bb7898e2efca1`
- NFCorpus MD5: `a89dba18a62ef92f7d323ec890a0d38d`

## Regression and performance

- Formatting and warnings-as-errors static analysis: PASS.
- Entire workspace contract suite: 88 passed, zero failed; the one release-size
  performance case is intentionally ignored in the normal suite and passed
  separately in release mode.
- 10,000 private memories plus 5,000 shared Editions: startup 2 ms, cold recall
  87 ms, capture p95 0 ms, steady recall p50/p95/p99 111/114/118 ms, concurrent
  maximum 12 ms: PASS.
- 64 Chinese golden queries, Agent isolation, disputed/expired filtering,
  migration, backup/restore, and six-tool MCP contracts: PASS.

## Release and safety gates

- Open-source independence, site, launch-surface, secret/private-path, and
  dataset/Vault exclusion checks: PASS.
- Temporary install, same-version repair, uninstall, reinstall, and final
  uninstall lifecycle: PASS.
- Stress: 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in one
  process, 8 concurrent clients, idle file-descriptor count 9, final Doctor
  healthy: PASS.
- RustSec audit of 194 locked dependencies and license policy: PASS.

No public corpus, query, relevance judgment, generated Vault, formal ART data,
or machine-private knowledge is included in this repository evidence.

## Reproduction

```bash
bash scripts/run_beir_retrieval_benchmark.sh <fixture-directory> <new-result.json>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p art-retrieval --test performance_contracts --release -- --ignored --nocapture
bash scripts/open_source_check.sh
bash tests/scripts/release-gate.sh
```

Benchmark source commit: `bfef99b`.
