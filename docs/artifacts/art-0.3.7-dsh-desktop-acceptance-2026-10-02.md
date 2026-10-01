# ART 0.3.7 local desktop acceptance — 2026-10-02

Status: local desktop adaptation accepted; ready for release review. No tag,
merge, push, release, Registry publication, or external announcement was made.
The formal host configuration was not switched to the candidate.

## Candidate and scope

Implementation commit: `d796cbc0c4de3a3552268352e63e970e06597a3b`.
Branch: `codex/art-dsh-desktop-adaptation`. Later documentation-only readiness
changes do not alter the accepted executable.

The installed official DSH desktop was `0.2.0-rc.2`; its application signature
passed verification. Acceptance used that application with disposable DSH,
browser, workspace, and ART homes. Research used documented public behavior,
CLI help/default configuration, and the installed UI; no host implementation
source was copied or inspected.

The Darwin arm64 archive was unpacked and its packaged installer installed ART
into the isolated home. The desktop then consumed the installed executable.
Source build, archive provenance, extracted binary, installed binary, and the
retained candidate binary had the same SHA-256:

`2115c39d1a72f2aa0fd27d47827fd4fbe39c4fa2cb31ddc9fcbb44a9f3471827`

Archive SHA-256:
`f47a000f589f38752ea0f4393e779a222221d10deec76851831ee3112b6f326f`.
The retained local candidate is named `0.3.7-dsh-desktop-20261002`.

## Changes

- `art integration dsh --desktop` previews a self-contained export. Applying
  requires an explicit new directory; exports never silently change host files.
- The patch uses literal executable/home paths and the desktop-owned bundled
  MCP client. The skill includes its private-memory value-standard reference.
- Existing destinations, absent Agent profiles, and invalid executable path
  encoding fail safely. Export directories/files are `0700`/`0600`.
- Desktop installation, restart, removal, and recovery instructions preserve
  unrelated host entries and ART data. The historical CLI integration remains
  compatible.
- Shared candidate hydration reuses one connection while preserving ranked IDs,
  hash/path checks, fresh revocation checks, and subsequent current-edition
  filtering. The existing 150 ms latency threshold was retained.
- Release metadata is prepared for 0.3.7; earlier public communication URLs are
  explicitly historical surfaces awaiting the 0.3.7 update.

## Verification

The complete release gate passed on the implementation candidate. It includes
formatting, strict Clippy, all-feature workspace tests, governance UI behavior,
migration, performance, generated artifacts, version consistency, plugin and
installer contracts, release assets, site/launch surfaces, stress, RustSec,
licenses, independence, and credential/path scans. Workspace tests: 242 passed,
0 failed. The single debug-ignored performance test was explicitly run in
release mode and passed; it was not omitted from acceptance.

A separate final-binary performance run retained the detailed measurements:

| Measurement | Milliseconds |
| --- | ---: |
| Startup | 2 |
| Cold recall | 88 |
| Capture p95 | 1 |
| Steady recall p50 / p95 / p99 | 44 / 54 / 56 |
| Concurrent maximum | 32 |

Stress passed: 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in
one process, 8 concurrent clients, 9 idle file descriptors, final Doctor `ok`.
Independent read-only code review found no remaining blocking issue.

The first gate exposed an existing hydration latency failure (p95 150 ms,
against a strict less-than-150-ms contract). Connection reuse fixed it. An
intermediate generated-artifact check also correctly rejected the old binary
checksum; the final manifest was regenerated from the actual 0.3.7 binary.
Neither check was weakened. A minimal shared fixture initially lacked its
Applicability section; completing that disposable fixture and rebuilding its
navigation restored alignment before the final desktop journeys.

## Real desktop journeys

These journeys used actual desktop controls to submit requests, receive tool
results, open local governance links, remove integration, and verify recovery.
Process presence and command-line success were only supporting evidence.

| Journey | Confirmed final result |
| --- | --- |
| Skill discovery/reference loading | `art-recall` and its bundled reference loaded in the installed desktop |
| Health/identity | ART 0.3.7; bound primary/secondary identities; vault/index `ok`; private/shared navigation aligned; no pending recoveries |
| Explicit capture | Synthetic scoped fact became active through `system_policy`; this was not human review |
| Idempotency | Original memory, revision 1, content hash, and receipt preserved; replay `true` even after removal/reinstallation |
| Exact private read | Primary could read its exact revision |
| Cross-Agent isolation | Secondary lexical and full-scan recall returned zero primary private memories; exact primary reference returned `ART_NOT_FOUND` |
| Shared transport | Both identities recalled/read the same hash-bound synthetic shared-channel fixture; secondary received no private source details |
| Route/fallback | Route returned bounded topics/references without bodies; full scan honored explicit selection; semantic/hybrid reported `semantic_unconfigured` and lexical fallback |
| Pending proposal | Exact source revision submitted; pending proposal did not become a shared Edition; desktop opened the exact proposal review page |
| Governance page | Final desktop page displayed the primary identity; delegation and automatic-memory checkboxes remained off |
| Abnormal disconnect | Task-owned MCP child changed from PID 88187 to 89621; fresh desktop health and recall succeeded |
| Removal/reinstallation | ART tools and skill disappeared; ordinary desktop answer still worked; restoring the installed candidate restored both tools and skill |
| Graceful closure | No isolated desktop host, MCP child, or governance service remained active |

Primary session: `session-9023270b-7381-4308-8413-bab1ddc84de9`.
Secondary session: `session-a74b9474-da61-4c1f-97e9-9620e88193f3`.

Synthetic private memory: `memory:artm_01M3WQJ2MQFXXD6JQYVME1YYDF@1`.
Receipt: `artir_01M3WQJ2MY6JMDZQG2X6R4WE1G`.
Content hash: `749298bf3cd0a25fb32be6da5bad6c54b22588df3f2476d5227e39bbde880cf1`.
Pending proposal: `artp_01M3WQKPJGR0RSJ83E3EATSHZH@1`.

Shared fixture: `knowledge:arke_desktop_fixture_20261002`.
Manifest SHA-256: `3d41a4a81d10815066598a920835bf4a1cb08aa958fef42982eed72f34057076`.
Markdown SHA-256: `85de1709c4d9e503228131c99923d45f1a54cb18f936d691922be93da1c0ccbf`.
The fixture was constructed only in disposable test storage, matching the
release performance fixture pattern. It proves desktop transport visibility,
not genuine human approval/publication. No formal ART Edition or policy was
edited. Genuine shared governance remains an operator action.

The final isolated Doctor returned `ok`, private integrity/search/navigation
aligned, two private memories/revisions, one pending proposal, one synthetic
shared projection, and no pending publish intent. The fixture's unchanged
knowledge-body digest remained valid after its Applicability field was completed.

## Evidence, cleanup, and boundaries

Retained private evidence includes the gate/performance logs, 39 bounded tool
receipts, Doctor output, and lifecycle/cleanup summaries. Receipt extracts omit
conversation text, capability URLs/tokens, credentials, and filesystem locators.

The original test directory (59,917,744 bytes) was moved into local Trash for
recovery after exact process/ownership checks. Its borrowed login-file symlink
was removed first. The formal login and desktop patch SHA-256 values were
unchanged, and the formal desktop was reopened. Residue verification marked
all 43 original temporary candidate groups no longer present at their original
locations. Source, Git history, the named candidate, and the pre-existing reused
Cargo cache were retained. Observation is task-scoped, not a machine-wide
cleanliness claim. Free space was approximately 60 GiB initially and 53 GiB at
handoff; quarantine is recoverable and does not reclaim those bytes.

Native Linux CI was not run locally. The local release gate checks Linux-labelled
archive/Registry packaging fixtures using the local binary; that does not prove
native Linux execution. Optional provider quality qualification and unrelated
fresh Codex UI journeys were outside this desktop adaptation scope. Existing
shared-governance positive/negative behavior passed the automated contracts;
no genuine human review or delegated publication was simulated in the formal
ART store. Publication must follow the normal reviewed main-commit release
pipeline after authorization.
