# ART Anchor Kind Contract Design

**Status:** approved on 2026-09-07

**Target:** local ART 0.3.4 candidate; no publication or formal-host replacement

## Problem

`art_memory_capture.anchors[].kind` is exposed as an unconstrained JSON string,
while the runtime accepts only eight internal values. Agents therefore cannot
discover the valid vocabulary from `tools/list`, and invalid intuitive aliases
such as `git` and `url` fail with the unhelpful message `invalid anchor kind`.

## Contract

The MCP input schema must expose exactly these values:

- `host_session_range`
- `user_statement`
- `file_snapshot`
- `git_object`
- `command_receipt`
- `test_receipt`
- `log_excerpt`
- `external_document`

The MCP layer must use the domain `AnchorKind` enum directly so serialization,
schema generation, and persistence share one source of truth. Unknown values
must fail at deserialization with an actionable error that includes the
canonical alternatives. Existing valid JSON requests remain compatible.

## Documentation

The bundled ART skill and host integration skills must list the canonical
values and give short selection guidance. They must not recommend aliases.

## Acceptance

The change is accepted only after a demonstrated RED/GREEN regression cycle,
focused and workspace tests, generated schema consistency, release gates,
packaged plugin validation, real stdio MCP requests, bounded Codex and DSH host
checks, lifecycle checks, and task-residue review. The installed public 0.3.3
runtime remains untouched during candidate acceptance.
