# ART launch FAQ

## Is ART a shared memory database?

No. Private memory is physically isolated per Agent. Cross-Agent material exists only as a human-reviewed Knowledge Edition.

## Does it save full transcripts?

No. ART is not a transcript store. Capture is for bounded, reusable conclusions with provenance and retention controls.

## Can an Agent publish knowledge by itself?

No. MCP exposes proposal drafting, not approval or publication. Those state changes require the local operator CLI.

## Which hosts are supported?

The first stable release targets Codex and DSH on macOS arm64 and Linux amd64.

## Does ART require a cloud account?

No. Storage, retrieval, review, and MCP transport are local. ART has no credential fields or telemetry dependency.
