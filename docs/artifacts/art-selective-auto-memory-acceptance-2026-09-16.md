# ART Selective Automatic Memory Candidate Acceptance

Date: 2026-09-16 (Asia/Shanghai)

## Decision

The opt-in selective automatic-memory implementation is accepted for a small
macOS ARM64 trial. It is not publicly released, tagged, pushed, or installed
over the user's formal ART runtime. The formal machine-wide setting remains
missing, which is the fail-closed default-off state.

Candidate source:

- branch: `codex/art-auto-memory`
- source commit: `d0dd78190965068bea70af673bf397bbb618d063`
- base commit: `b86ad22e6be2722f5f3cb6e4661dcafd30530192`
- package: `agent-recall-trail_0.3.5_darwin_arm64.tar.gz`
- package SHA-256:
  `62639c8ab1e9a19a067cd323c38feccbc12aecd02ac962b6b3af8254013109a4`
- embedded binary SHA-256:
  `62bd62f92b2d85a921c4421277d2d1234a13ec86d0d112b80a01edacc94c02b5`
- package provenance target: `darwin_arm64`

## Delivered behavior

- A persistent, machine-wide setting defaults and fails closed to disabled.
  Only the authenticated governance page can change it; MCP exposes read-only
  status. It is distinct from delegated shared-knowledge governance.
- Codex `UserPromptSubmit` records only a bounded correction signal while
  enabled. `Stop` classifies the final Agent message without reading a
  transcript and requests at most one bounded continuation.
- Trigger receipts enforce one turn identity, one candidate index, three
  accepted triggers per session, a ten-minute cooldown, replay safety, and
  restart persistence.
- `art_memory_candidate_submit` binds the current Agent and accepts Candidate
  content only. It rechecks the global setting at submit and activation.
- The policy rejects sensitive content, verifies current file and Git sources,
  requires scoped command/test receipts, automatically activates only safe
  non-conflicting evidence, and leaves uncertain or replacement content for
  human review.
- The governance page shows global status, host support, limits, Hook evidence,
  capture outcome, pending count, sources, and bound-Agent confirm,
  edit-confirm, and reject operations.
- DSH retains explicit capture and recall; it does not emulate the Codex Hook.

## Automated verification

The final `tests/scripts/release-gate.sh` completed with `release gate: ok` on
the exact source commit contents. The gate included:

- `cargo fmt --all --check`;
- Clippy for all workspace targets and features with warnings denied;
- 195 enumerated Rust tests (the large release performance contract runs in
  release mode after its normal debug-mode skip);
- migration, backup/restore, generated-schema, version, plugin-surface,
  install/repair/uninstall, site, launch, open-source, and independent-product
  checks;
- 500 graceful MCP sessions, 100 abnormal disconnects, 1,000 queries in one
  process, and eight concurrent clients; final doctor status was `ok`;
- RustSec audit of 333 locked dependencies with zero remaining advisories;
- source credential-pattern scan with zero matches.

The classifier suite contains 65 literal Chinese/English positive, negative,
sensitive, quoted-memory, unresolved-failure, and malformed examples. A fresh
100-process non-trigger measurement on the final release binary produced:

| Measure | Result |
| --- | ---: |
| p50 | 5.821 ms |
| p95 | 6.708 ms |
| maximum | 9.923 ms |
| requested continuations | 0 |

This satisfies the local p95 target of at most 100 ms, and non-trigger rounds
request no additional model work.

## Real host acceptance

### Codex

- A normally launched isolated Codex task loaded the candidate plugin, listed
  its nine ART tools, and called `art_health` through the live MCP server. The
  missing config reported automatic memory disabled.
- With a task-owned enabled config, a negative Stop case produced one Hook
  classification but the Agent returned `no worth recording`; the isolated
  Vault retained zero memories and zero submission receipts.
- A positive task read and independently hashed a bounded JSON result. Stop
  requested exactly one continuation. The Agent submitted candidate index 0,
  and policy `art.auto-memory.policy.v1` activated it as
  `artm_01M2JZ77TF8K5C2VBKZQCHT991@1` with reason
  `verified_scoped_receipt` and policy actor `system_policy`.
- A subsequent lexical recall returned that exact subject reference, and exact
  read returned the decision, scope, limitations, and revision 1.
- This journey exposed an unconstrained `scope_type` schema that encouraged an
  invalid `project` value. A failing MCP contract test reproduced it; the
  schema and deserializer now expose exactly `session`, `repository`,
  `workspace`, `machine`, and `user`. The positive journey then passed.

Automated positive Hook acceptance used Codex's documented
`--dangerously-bypass-hook-trust` test flag after the task-owned plugin source
had been reviewed. A separate normal launch succeeded without the bypass and
did not display a trust prompt, so the presence of an interactive first-trust
dialog was not claimed as tested.

### Codex in-app browser

The ART governance page loaded in Codex's in-app browser from a loopback-only,
short-lived Settings session. It visibly showed:

- the machine-wide scope and separation from delegated publication;
- enabled state in the isolated fixture, policy version, three/session limit,
  and ten-minute cooldown;
- distinct latest Hook `accepted` and capture `activated` evidence after the
  real Codex journey.

No 403 occurred. Authenticated setting mutation, CSRF, hostile Origin,
persistence, and candidate review operations were exercised by the real HTTP
governance contract tests. The browser setting was not clicked during the
automated UI inspection because that would be an additional interactive local
setting confirmation; the isolated configuration fixture supplied the visual
state.

### DSH

DSH `0.1.1-rc.2` composed the ART overlay against a task-owned home. On the
machine's already configured profile, an id-targeted task override safely
replaced the existing `art-memory` entry to avoid a duplicate loader ID without
editing user configuration. A real headless DSH task then:

- called `art_health` for bound Agent `dsh-primary`;
- explicitly captured active memory
  `artm_01M2JZAMQ3SYKNGMYKZ3ZYKC81@1`;
- recalled the exact title and subject reference;
- left automatic trigger and automatic submission receipt counts at zero.

## Candidate package acceptance

The retained candidate archive contains the ARM64 macOS binary, install
script, license, provenance, Codex plugin manifest, MCP declaration, Hook
configuration, launcher, and ART skill. A clean installation from the archive
reported `art 0.3.5`; Agent creation succeeded and `art doctor` returned
top-level status `ok`.

The repository's aggregate release-asset gate also passed. A standalone Linux
candidate is not delivered here: the host has no Linux Rust target installed
and its Docker daemon is not running. Relabeling the macOS binary as Linux
would not be valid evidence. Cross-platform publication remains a later formal
release responsibility.

## Security and corrections found during acceptance

- `rustls` was upgraded from 0.23.43 to 0.23.45 after RustSec
  `RUSTSEC-2026-0285` blocked the first gate.
- Secret-detection fixtures were split into source-safe compile-time fragments
  so tests still exercise the forbidden runtime values without shipping
  credential-like literals.
- The generated-artifact gate now binds `docs/artifacts/checksums.txt` to the
  final remapped release binary, preventing an earlier build's hash from being
  mistaken for candidate provenance.

## Residue and release boundary

The source branch/worktree, this report, implementation/design documents, and
candidate archive are intentionally retained. The task-owned Codex marketplace
and candidate plugin cache entry were removed, the loopback governance process
and browser tab were closed, and the isolated ART/DSH homes, extracted package,
and temporary receipts were moved from `/tmp` to the user's Trash as one
recoverable 43 MB directory.

The bounded conclusion was stored and exactly read back as formal private
memory `artm_01M2K08TH04C230Z6WYESE07M9@1`. The original recoverable residue
observation could not be finalized after long-task context compaction because
its secret owner handle was no longer available; manual process, port, plugin,
filesystem, and Git checks supplied the final cleanup evidence instead. No ARE
candidate path was resolved or deleted.

No public tag, release, registry upload, remote branch, formal ART installation,
or formal switch mutation is authorized or performed by this candidate work.
