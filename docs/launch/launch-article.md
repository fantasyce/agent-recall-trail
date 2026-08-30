# ART v0.1.0: private Agent memory, reviewed shared knowledge

Coding Agents need continuity, but a shared transcript bucket is the wrong authority. ART gives each Agent a physically separate private Recall Trail, then makes reusable knowledge cross the boundary only through a human-reviewed, immutable Knowledge Edition.

ART is not a transcript store, prompt injector, cloud memory service, or autonomous publisher. It is a local Rust runtime for Codex and DSH with six bounded MCP tools. Agents can recall, capture, read, give feedback, and draft proposals; only a human operator can approve or publish shared knowledge.

The v0.1.0 release includes native macOS arm64 and Linux amd64 builds, an Apache-2.0 Codex plugin, deterministic Markdown migration with reconciliation receipts, source-locked knowledge proposals, and a private-by-default SQLite/FTS5 storage model.

Start with the release archive, verify `SHA256SUMS`, run the installer, create an Agent identity, and connect the stdio MCP server. The architecture and threat model are public so teams can evaluate the boundary rather than trust a slogan.
