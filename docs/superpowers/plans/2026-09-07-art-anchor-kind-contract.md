# ART Anchor Kind Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce a locally accepted ART 0.3.4 candidate whose MCP schema, runtime, generated artifacts, and skills share one discoverable source-anchor vocabulary.

**Architecture:** Reuse `art_domain::anchor::AnchorKind` as the public MCP input type and remove the duplicated string parser. Lock the wire contract through schema and deserialization tests, then propagate the canonical vocabulary through bundled skills and versioned release surfaces.

**Tech Stack:** Rust, serde, schemars, rmcp, shell release gates, Codex plugin packaging, DSH overlay

**Spec:** `docs/superpowers/specs/2026-09-07-art-anchor-kind-contract-design.md`

## Global Constraints

- The canonical anchor vocabulary is exactly the eight values listed in the spec.
- Valid 0.3.3 JSON requests remain compatible.
- Invalid aliases fail before persistence and identify the allowed vocabulary.
- Formal Codex and DSH installations remain on public 0.3.3 during candidate testing.
- All test data, processes, ports, and packages are task-owned and cleaned after acceptance.
- No tag, push, registry publication, release creation, or public marketplace mutation.

---

### Task 1: Lock the MCP input contract with failing tests

**Files:**
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`
- Test: `crates/art-mcp/tests/mcp_contracts.rs`

**Interfaces:**
- Consumes: `ArtMcpServer::tool_schema_json`, `MemoryCaptureInput`, `SourceAnchorInput`
- Produces: regression assertions for schema discovery and serde rejection

- [ ] **Step 1: Add a schema regression test**

Extract `art_memory_capture.inputSchema.$defs.SourceAnchorInput.properties.kind`
and assert that its inline or referenced enum equals the eight canonical values.

- [ ] **Step 2: Add a deserialization regression test**

Deserialize a complete `MemoryCaptureInput` JSON object with `kind: "git"`,
assert failure, and require the error to mention `git_object` and
`external_document` so the caller receives actionable alternatives.

- [ ] **Step 3: Run the focused tests and record RED**

Run:

```bash
cargo test -p art-mcp --test mcp_contracts anchor_kind -- --nocapture
```

Expected: the schema assertion fails because `kind` is only `type: string`.

### Task 2: Replace the string/parser split with the domain enum

**Files:**
- Modify: `crates/art-mcp/src/lib.rs`
- Modify: `crates/art-mcp/tests/mcp_contracts.rs`

**Interfaces:**
- Consumes: `art_domain::anchor::AnchorKind`
- Produces: `SourceAnchorInput { kind: AnchorKind, ... }`

- [ ] **Step 1: Type the input field**

Change `SourceAnchorInput.kind` from `String` to `AnchorKind`.

- [ ] **Step 2: Remove duplicate conversion**

Pass `anchor.kind` directly to `SourceAnchor::new_with_source` and delete
`parse_anchor_kind` from the MCP crate.

- [ ] **Step 3: Update Rust test fixtures**

Replace string construction with explicit `AnchorKind` variants while leaving
JSON-wire tests in snake_case.

- [ ] **Step 4: Run focused tests and record GREEN**

Run the Task 1 command and require all matching tests to pass.

- [ ] **Step 5: Prove the regression test is meaningful**

Temporarily revert only the typed field and direct conversion, rerun the schema
test to observe failure, restore the fix, and rerun it to pass.

### Task 3: Teach all shipped skills the canonical vocabulary

**Files:**
- Modify: `plugin/agent-recall-trail/skills/agent-recall-trail/SKILL.md`
- Modify: `integrations/codex/art-recall/SKILL.md`
- Modify: `integrations/dsh/art-recall/SKILL.md`
- Modify: `tests/scripts/test_plugin_surface.sh`

**Interfaces:**
- Consumes: the eight-value MCP contract
- Produces: concise selection guidance in every distributed skill surface

- [ ] **Step 1: Add failing packaged-skill assertions**

Require every shipped skill to contain the eight canonical values and reject
guidance that presents `git` or `url` as valid kinds.

- [ ] **Step 2: Run the plugin surface test and record RED**

```bash
ART_BIN=target/debug/art bash tests/scripts/test_plugin_surface.sh
```

Expected: failure because the skills do not enumerate the vocabulary.

- [ ] **Step 3: Add minimal reference guidance**

List the eight values once per shipped skill and distinguish session/user/file,
Git, command/test/log, and external-document evidence.

- [ ] **Step 4: Run the plugin surface test and record GREEN**

Require exit code zero and inspect the packaged skill, not only source text.

### Task 4: Move versioned candidate surfaces to 0.3.4

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `plugin/agent-recall-trail/.codex-plugin/plugin.json`
- Modify: `scripts/install.sh`
- Modify: `tests/scripts/test_release_version.sh`
- Modify: `tests/scripts/test_plugin_surface.sh`
- Modify: `tests/scripts/test_install_lifecycle.sh`
- Modify: `tests/scripts/release-gate.sh`
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/publish-mcp.yml`
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `site/index.html`
- Modify: `docs/launch/launch-article.md`
- Modify: `docs/launch/launch-manifest.json`
- Modify: `scripts/test_launch_surface.sh`

**Interfaces:**
- Consumes: workspace package version and release templates
- Produces: internally consistent unpublished 0.3.4 candidate

- [ ] **Step 1: Change version tests to expect 0.3.4 and observe RED**

Run `bash tests/scripts/test_release_version.sh` against the current binary and
require a version mismatch.

- [ ] **Step 2: Update active release surfaces**

Change active version declarations and current launch copy to 0.3.4. Preserve
historical 0.3.3 specs and acceptance records unchanged.

- [ ] **Step 3: Regenerate the lockfile and validate versions**

Run `cargo check -p art-cli`, build the debug binary, then run release-version,
plugin-surface, and launch-surface checks.

### Task 5: Regenerate and inspect governed artifacts

**Files:**
- Modify: `docs/artifacts/mcp-tools.schema.json`
- Modify: `docs/artifacts/checksums.txt`
- Modify when generated by the established scripts: license/SBOM artifacts

**Interfaces:**
- Consumes: final tool schema and dependency graph
- Produces: checked-in artifacts matching source

- [ ] **Step 1: Generate schemas and release artifacts through repository scripts**

Use the existing artifact generation path; do not hand-edit generated JSON.

- [ ] **Step 2: Inspect the generated anchor enum**

Require exactly eight values under `SourceAnchorInput.kind` and no stale
unconstrained string-only schema.

- [ ] **Step 3: Re-run artifact and checksum validation**

Require source-controlled generated outputs to be stable on a second run.

### Task 6: Strict local candidate acceptance

**Files:**
- Create: `docs/artifacts/art-0.3.4-anchor-kind-acceptance-2026-09-07.md`
- Test: workspace, release scripts, packaged plugin, stdio MCP, Codex, DSH

**Interfaces:**
- Consumes: exact candidate commit and generated packages
- Produces: local acceptance record with evidence ladder and explicit skips

- [ ] **Step 1: Run focused quality gates**

Run formatting, warnings-as-errors clippy, all `art-mcp` tests, and all workspace
tests with one test thread.

- [ ] **Step 2: Run the complete release gate**

Run `bash tests/scripts/release-gate.sh` and require exit code zero, including
performance, audit, license, independence, secret, lifecycle, and stress gates.

- [ ] **Step 3: Exercise real stdio MCP requests**

Against the built candidate in a task-owned ART home, inspect `tools/list`,
capture/read one memory with `git_object`, capture/read one with
`external_document`, verify idempotent replay, and prove `git` and `url` fail
without creating records while their errors list canonical alternatives.

- [ ] **Step 4: Verify packaged and host boundaries**

Build the MCPB/release assets, install only into an isolated home, start the
packaged server, and verify the same schema and behavior. Exercise a bounded
Codex invocation against the candidate server configuration and a bounded DSH
overlay/page journey without replacing the public 0.3.3 installation.

- [ ] **Step 5: Record evidence and review residue**

Record source/package/install/runtime/host evidence, test counts, versions,
temporary roots, processes, ports, and any skipped checks. Remove only exact
task-owned temporary roots and stopped processes, then verify both Git trees,
worktree inventory, disk usage, and formal 0.3.3 installations.

- [ ] **Step 6: Final verification**

Rerun the focused regression, version checks, schema comparison, and Git status
after cleanup. Do not claim completion from earlier outputs.
