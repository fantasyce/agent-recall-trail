# ART v0.3.1 release acceptance — 2026-09-06

## Decision

**PASS for publication.** ART v0.3.1 is a compatibility patch for Codex
plugin startup in GUI and restricted SSH environments. It does not change the
memory, knowledge, review, storage, retrieval, or six-tool MCP contracts.

The accepted change replaces the plugin's bare `art` command with a
package-relative POSIX launcher. The launcher preserves an explicitly
configured PATH installation, falls back to the installer's standard
`$HOME/.local/bin/art` link, uses `exec`, and neither sources a login shell nor
initializes ART state. The Codex skill now names the actual feedback tool and
documents existing recall bounds. Codex and DSH guidance describe the same
parameter contract.

## Review findings

- The original failure was reproduced under a host PATH without the
  user-local executable directory.
- The launcher, manifest, skill, release workflow, installer, version surfaces,
  and release-gate isolation behavior received a final line-by-line review.
- Two release-gate paths that assumed a repository-local Cargo target were
  corrected so full acceptance can use task-owned build storage.
- Three raw desktop-session reports containing local paths and session detail
  were excluded from the public tree. The retained plugin acceptance report is
  bounded and path-neutral.
- No remaining release-blocking defect was found.

## Exact v0.3.1 candidate gates

- Formatting and all-workspace, all-target, all-feature Clippy with warnings
  denied: PASS.
- Locked, all-feature workspace tests: 119 passed, zero failed; the one ignored
  release performance contract was executed separately and passed.
- Plugin launch contracts: eight passed, covering restricted PATH, fallback,
  PATH precedence, spaces, argument preservation, unavailable installations,
  missing Agent behavior, clean stderr, EOF shutdown, and reconnect.
- Exact six-tool MCP surface and skill/tool name agreement: PASS.
- Installer lifecycle, migration, site, launch surface, open-source identity,
  license metadata, private-path, and credential scans: PASS.
- Stress: 500 graceful sessions, 100 abnormal disconnects, 1,000 queries in one
  process, eight concurrent clients, nine idle file descriptors, and final
  Doctor health: PASS.
- RustSec: 301 locked dependencies scanned against 1,239 advisories with
  warnings denied: PASS.
- macOS arm64 candidate binary SHA-256:
  `f5dc793d4adfa3d0d41b6800178d18e7f024bbda319b32546946c5ed4c8a022c`.
- Temporary path-neutral macOS archive SHA-256:
  `fc82222aba58e9c687912d95ee6d44aaa612b85c16bca54c97f0640d8c4c485a`.

Earlier real-host acceptance used the repaired installed launcher with both
ordinary and managed Codex homes. Two final fresh sessions each completed
seven native ART calls with zero failures and retrieved exact private revisions
and the same committed shared Knowledge Edition. A separate operator session
also accepted native desktop access. Formal ART counts, indexes, navigation,
and hashes remained healthy after the read-only acceptance.

## Publication and installation boundary

The release tag must resolve to the exact accepted `origin/main` commit. GitHub
Actions builds macOS arm64 and Linux amd64 archives, assembles and verifies the
MCP bundle, publishes the GitHub release, and then publishes the same bundle to
the MCP Registry. The exact public macOS archive must be downloaded and
checksum-verified before it replaces the formal local executable.

After publication, Codex receives the plugin files from that exact released
tree and DSH receives its ART skill plus a persistent stdio MCP profile binding
to `dsh-primary`. Existing formal ART data is preserved. Final smoke acceptance
must verify version, Agent binding, private recall, shared knowledge recall,
exact read, and clean shutdown in both hosts.

The public CI matrix supplies Linux acceptance. Optional external embedding is
not configured and is not part of this patch's release claim; lexical retrieval
remains the default, with semantic fallback contracts covered by the Rust
suite. This report authorizes publication only after protected-branch CI passes
on the submitted commit.
