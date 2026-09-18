# Unified memory intake: local candidate acceptance

Date: 2026-09-17 (Asia/Shanghai). Platform: macOS ARM64.

## Decision and evidence boundary

The final candidate is installed locally and the source, package, isolated
lifecycle, real Codex ordinary-intake and Stop Hook, and DSH regression checks
pass. Complete host acceptance is **not claimed**: visual in-app governance
interaction remains unverified. Existing active App turns also
retain older MCP children; a fresh App connection demonstrably uses the final
binary. No public release, push, tag, or registry publication was performed.

## Exact candidate

| Item | Identity |
| --- | --- |
| Source commit | `8637a1c281e7a05ba7c82baf1da8f07c7434338f` |
| Branch | `codex/art-auto-memory` |
| Candidate directory | task-owned local candidate storage (not published) |
| Archive | `agent-recall-trail_0.3.5_darwin_arm64.tar.gz` |
| Archive SHA-256 | `8329e5e7bd308858a3fb30cbe721f265fb33daf04fcc9a8634fd1861a8590ee6` |
| Embedded/installed binary SHA-256 | `7a3d7abc00b85fbbfaaaf0cd2675f0ca59131524771db602421295951cfa2efe` |
| Original installed binary SHA-256 | `62bd62f92b2d85a921c4421277d2d1234a13ec86d0d112b80a01edacc94c02b5` |
| Version / target | `0.3.5` / `darwin_arm64` |
| Package provenance | `art-install-provenance/1.0`, exact source commit and binary hash above |
| Automatic config / policy | `art.auto-memory.config.v1` / `art.auto-memory.policy.v1` |
| Intake receipt | `art.memory-intake.receipt.v1` |

Version 0.3.5 is intentionally unchanged for this local candidate; the commit
and hashes distinguish it from earlier 0.3.5 bytes. This report is a later
documentation-only commit, not a claim that its commit is the packaged source.
The retained archive was extracted again: binary hash matched, plugin files
matched the formally installed cache recursively, its bundled installer ran,
and its installed executable reported doctor status `ok`.

## Defect found and final gates

Real Codex capture supplied a correct digest as `sha256:<hex>`. The previous
verifier accepted only bare hex, incorrectly leaving verified file evidence
pending with `source_not_current`. Commit `8637a1c` adds two regression tests
before fixing shared SHA-256 verification for files and command/test receipts.
The positive test failed with `PendingReview` instead of `Activated` before
the fix. Nine origin/anchor combinations then passed; twelve negative
combinations still reject wrong algorithms, repeated prefixes, wrong digests,
and changed source bytes. Stored anchors and replay/evidence hashes are not
normalized or rewritten.

After rebuilding and reinstalling the corrected bytes:

- Focused agent-store/CLI/MCP suites: 154 passing tests; format and Clippy pass.
- Full `tests/scripts/release-gate.sh`: `release gate: ok`; 237 workspace Rust
  tests pass. The one normal debug-mode performance skip is explicitly rerun
  and passes in release mode, not omitted coverage.
- Shipped governance JavaScript event-handler behavior passes, alongside
  migration, generated artifacts, plugin surface, archive, install, site,
  launch, open-source, and independent-product checks.
- Stress: 500 graceful sessions (46.181 s), 100 abnormal disconnects, 1,000
  same-process queries (1.561 s), eight clients, nine idle descriptors, doctor
  `ok`. RustSec checked 333 dependencies against 1,247 advisories successfully.

The first aggregate rerun failed because temporary candidate archives were
inside the source-scanned tree. They were relocated outside it, and the whole
gate was rerun successfully. No scanner exception was introduced. Linux
runtime acceptance was not performed on this macOS host; aggregate archive
fixtures are not evidence of a working Linux release binary.

## Installation and lifecycle

Fresh final-byte isolated homes passed clean install, same-version repair,
upgrade from the original installed binary, rollback, re-upgrade, and
uninstall. An original private revision survived migration and exact read;
one pre-v4 migration snapshot was produced. Rollback was an offline,
full-home snapshot restoration plus the original executable, never direct
SQLite editing. Re-upgrade retained that revision again. Uninstall removed
only executable/link and preserved the Vault. This does not authorize replacing
a live formal Vault with an older snapshot containing fewer memories.

Formal installation used the normal binary installer and
`codex plugin add agent-recall-trail@personal`. The source plugin directory and
installed 0.3.5 cache match the packaged plugin. Both installed Hooks were
inspected and trusted through the App server's normal persisted Hook-trust
configuration path; `hooks/list` reports enabled/trusted, no warnings/errors.
No Hook-trust bypass flag was used.

The existing formal switch was preserved byte-for-byte: enabled, config
version 1, three captures/session, 600-second cooldown. Its before/after SHA-256
is `3e57e3c117771d17414c221f435fb150e3466f4787dfa69603c274d465c03705`.
Only task-owned homes received automated fixtures or setting changes. Formal
private memory was not populated with acceptance fixtures or directly edited.

Codex Desktop/CLI 0.154.0 accepted `config/mcpServer/reload`. A new ephemeral
App-server thread called formal `art_health`: version 0.3.5, Vault/index `ok`,
zero pending recoveries, both navigation projections aligned, expected bound
Agent, preserved automatic setting, and the unified capture schema present.
Its new MCP child's mapped executable inode equaled the final installed
binary (347845316), while existing active-turn children mapped earlier inodes.
The ephemeral thread was unsubscribed. Active user turns were not forcibly
terminated: full retirement of their older children remains a next-turn or
controlled App-restart boundary, not a completed all-process reconnect.

## Real consuming-agent acceptance

All identifiers below are from task-owned private Vaults. Content was a bounded
verified local rollback procedure, not a user preference. Source evidence was
independently hashed; no raw transcript was stored as memory.

| Journey | Final-byte result |
| --- | --- |
| Codex proactive, Hooks/plugins disabled | `agent_initiated`, `activated`, `memory:artm_01M2QC080Q860QJJ8QKG53HNX1@1` |
| Codex explicit, automatic switch off | `user_requested`, `activated`, `memory:artm_01M2QC0MCBF9RF5D035G2NRQE4@1`; lexical recall and exact read pass |
| Codex App-server mundane turn | Real Stop Hook completes in 27 ms; no continuation or memory tool call |
| Codex App-server verified decision | One real Stop continuation; `hook_triggered`, `activated`, `memory:artm_01M2QDR39J35C1FHD68Y982SSP@1` |
| DSH proactive, no Hook | `agent_initiated`, `pending_review`, `memory:artm_01M2QC3HBA916D6N3AK6CTTFGD@1` |
| DSH explicit, no Hook | `user_requested`, `activated`, `memory:artm_01M2QCQ5B36DKADD2XTAY10KQK@1`; lexical recall and exact read pass |

Ordinary Codex and DSH attribution remains Agent-asserted/degraded, not trusted
human identity. DSH 0.1.1-rc.2 used an id-targeted profile overlay with the
formally installed executable and task-owned storage; user profile files were
unchanged. Its proactive Agent omitted `source_digest`, so current source
verification correctly failed closed. The explicit task included the digest
and activated. This is not proof that every proactive Agent will supply a
complete source anchor. DSH's prose blamed the temporary path; inspecting its
actual anchor showed the missing digest was the relevant limitation.

Normal Codex CLI Hook trials completed their mundane/verified-decision main
turns but did not expose ART plugin MCP tools or produce Hook-run/trigger rows
in the isolated home. Plugin enablement, local marketplace type, persisted
trust hashes, and trusted workspace overrides were tried. Loading the full
App configuration directly was separately blocked by the CLI's unsupported
configured `cua_repl` transport. Therefore neither main-turn completion nor
absence of writes counts as a real Hook no-op/positive continuation pass.
The exact underlying CLI loading cause was not resolved in this task; the
subsequent bounded App-server journey below closes the required real Hook
acceptance without relying on that CLI configuration.

A final isolated Codex App-server used a task-owned Codex configuration,
normal installation of the exact installed plugin bytes, persisted trust from
its own `hooks/list` hashes, and an ART launcher explicitly bound to a task-owned
Vault. A read-only authentication-file link reused the existing sign-in without
copying credentials. The observed MCP process arguments proved isolation
before any real turn ran. The [documented App-server lifecycle](https://learn.chatgpt.com/docs/app-server)
was used for initialization, thread/turn events, and unsubscribe.

The mundane turn's actual Stop Hook completed with no feedback or continuation
(27 ms), and no memory tool call. The following verified-decision turn produced
one `hook/completed` blocked event (50 ms) and one real Hook prompt containing
trigger `artamt_01M2QDQJKR9BSMDKEXSY3BT7VW`. The Agent submitted only candidate
index 0 through `art_memory_candidate_submit`; correct `sha256:` file evidence
activated under `verified_scoped_receipt`. Intake receipt
`artir_01M2QDR39VVSCVA16P8QQT11C2` records host-supplied session and turn, not
degraded attribution. The final Stop completed in 24 ms without a second
continuation. Installed diagnostics show exactly one Hook intake, zero ordinary
or explicit intakes, zero pending candidates, and shared budget used=1. Exact
MCP read after the turn returned Active revision 1. No trust bypass or product
code change was needed. This is real host execution, unlike the synthetic
protocol checks below.

Supplemental final installed CLI/MCP protocol tests with explicitly synthetic
task-owned host events pass: non-trigger returns `continue`; a genuine emitted
trigger receipt admits one index-0 Hook candidate; exact replay is stable; and
turning the isolated switch off after the trigger makes submission `disabled`
with no memory. Candidate `artm_01M2QCYJYY7J2S1T4MKJFYV689@1` remained pending.
These are protocol checks, not substituted evidence of a real Codex continuation.

## Governance and boundary checks

Live installed governance sessions were created for Settings, Pending, and
Audit. Authenticated loopback HTTP exercised the actual installed routes:

- Exact ordinary-input replay returns the original receipt; new-key identical
  content returns `duplicate`, the original memory reference, and no budget
  association. Shared automatic budget remained used=1.
- Settings exposes both automatic origins, Codex Agent+Hook support, and DSH
  Agent-only support. Switching off yields automatic `disabled`; an explicit
  uncertain submission still yields `pending_review`; isolated state is restored.
- Candidate edit-confirm produces Active revision 2; a proposed replacement
  preserves the current revision until review; rejecting that proposal keeps
  revision 2 and appears in the audit review list.

These automated isolated route actions use the route's fixed human-UI actor
label, but are explicitly **not a claim of a human click**. The served JavaScript
SHA-256 is `72b36707d765235246f8e0771a753c9294aa53b308aeedfe2982d37778c21d7a`;
the final release gate separately exercises its real event handlers.

In-app visual interaction is unverified. Computer-use inventory timed out
repeatedly, including after a temporary display wake and a final retry; an
in-app-tab creation attempt reported the browser unavailable. Current screen
preferences did not establish a password lock. No visual pass, manual unlock,
or successful browser control is inferred from those errors.

## Residue and remaining work

The final 9.1 MB archive, installed runtime, source worktree, this report, and
bounded local handoff evidence are retained. Task-owned temporary homes,
transcripts, test identities, lifecycle copies, and obsolete archive (229 MB)
were moved together to Trash after stopping their isolated governance server;
they are recoverable. Exact-root open-file checks were empty before the move,
and the task's loopback listener was gone. Available disk was approximately
55 GiB before and 54 GiB after this build/test work; moving to Trash is recovery,
not a claim of reclaimed disk space.
The final bounded Hook closure additionally stopped its isolated App-server
cleanly, removed only its task-owned authentication symlink (not its target),
and moved its 73 MB home/workspace/log root to Trash after empty process/open-file
checks. The final candidate and installed binary hashes are unchanged.
The reused Cargo cache is pre-existing/shared and is not removed. No user data,
formal Vault, branch, remote state, or active App child is deleted.

ARE tools were unavailable in the session; bounded process, port, exact-path,
disk-usage, and Git checks supply the cleanup evidence, without pretending to
be an ARE receipt. Remaining acceptance requires visual in-app review and
retirement/reconnect of old active-turn
children at a safe host boundary. Those gaps block an unconditional complete
host-acceptance claim, not the recorded lower-level successes.
