# ART launch FAQ

## Is ART a shared memory database?

No. Private memory is physically isolated per Agent. Cross-Agent material exists only as a human-reviewed Knowledge Edition.

## Does it save full transcripts?

No. ART is not a transcript store. Capture is for bounded, reusable conclusions with provenance and retention controls.

## Can an Agent publish knowledge by itself?

No. MCP exposes proposal drafting, not approval or publication. Those state changes require the local operator CLI.

## Which retrieval mode is the default?

Lexical. Users may explicitly choose governed full scan, semantic, or hybrid retrieval on each request.

## Does ART provide an embedding model?

No. Embedding is an optional provider-neutral adapter. The user selects, operates, and evaluates any compatible endpoint. If it is not configured or healthy, ART remains usable and semantic requests fall back to lexical with explicit diagnostics.

## Which hosts are supported?

ART targets Codex and DSH on macOS arm64 and Linux amd64.

## Does ART require a cloud account?

No. Storage, review, lexical retrieval, full scan, and MCP transport are local. Only an explicitly enabled semantic path contacts the operator-selected HTTPS endpoint.
