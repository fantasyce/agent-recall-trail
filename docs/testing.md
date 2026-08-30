# Testing and acceptance

## Automated gates

`bash tests/scripts/release-gate.sh` runs formatting, warnings-as-errors static analysis, all workspace tests, the release performance dataset, the lifecycle/pressure gate, dependency audit, license policy, product-independence checks, and a secret-pattern scan. CI runs on macOS arm64 and Linux x86_64.

Contract tests cover:

- canonical Agent IDs, path containment, schemas, typed payloads, lifecycle edges, anchors, and assurance;
- physical Vault isolation, identity mismatch, capture/revision/export idempotency, migration race, mid-transaction rollback, read-only/simulated disk-full behavior, eight concurrent writers, schema fail-closed, source invalidation, and operator lifecycle;
- proposal source locks, human-only review, stale-source blocking, immutable publication, six crash-boundary recovery states, lifecycle event reconciliation, corruption detection, Git non-mutation, revocation, and path traversal;
- deterministic allowlisted backup, corruption and link rejection, encrypted
  control-authority round trips, atomic empty-home restore, and fresh-clone
  projection rebuild;
- exact/Jieba/bigram retrieval, candidate filtering, private/shared separation, and 64 Chinese golden queries;
- six-tool MCP success paths, strict object output schemas, revision staleness, DB busy, shutdown, no-persist rejection, legacy/current protocol initialization, JSON-only debug stdout, and EOF/signals;
- CLI confirmation boundaries, config priority, deep Doctor diagnostics, private permissions, integration previews, reviewable Markdown scan, safe copy, and import/export behavior;
- thin Across Context contract expiry, no-persist, and invalidation behavior without an AAA adapter.

## Real-host E2E

Acceptance uses task-owned storage and temporary opt-in configuration only:

1. Codex primary health, capture, and recall.
2. Codex secondary negative private recall.
3. DSH primary health, capture, and recall.
4. DSH secondary negative private recall.
5. Codex proposal, local human approval/publication, then shared Edition recall from both secondary Agents.
6. Skill discovery and failure-boundary behavior in both hosts.
7. Graceful EOF, abnormal disconnect, concurrency, repeated-query, file-permission, source-read-only, and residue checks.

The reproducible local stress artifact runs 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in one process, 8 concurrent clients, an idle-FD ceiling of 16, and a final healthy Doctor. The release performance dataset contains 10,000 private memories and 5,000 shared Editions; reports must retain startup, cold-index, capture p95, steady recall p50/p95/p99, and concurrent maximum separately.

Passing source tests alone is not host E2E. A host run passes only when the real installed Codex or DSH invokes the release ART child, returns the expected bound identity, and proves the requested private/shared visibility result.
