# ART v0.3.0: progressive recall with user-selected retrieval

Coding Agents need continuity, but private experience and shared knowledge do not have the same authority. ART gives each Agent a physically separate private Recall Trail, then allows stable material to cross the boundary only as a human-reviewed, immutable Knowledge Edition.

ART is not a transcript store, prompt injector, cloud memory service, or autonomous publisher. It is a local Rust runtime for Codex and DSH with six bounded MCP tools. Agents can recall, capture, read, give feedback, and draft proposals; only a human operator can approve or publish shared knowledge.

The v0.3.0 release adds progressive `route -> recall -> read` retrieval and four explicit modes behind the same recall API. Lexical remains the zero-configuration default. Governed full scan evaluates every eligible canonical record. Semantic and hybrid modes use a user-operated OpenAI-compatible embedding endpoint and disposable local projections only when explicitly selected. An unavailable optional provider falls back to the unchanged lexical result with visible diagnostics.

ART does not bundle, choose, train, or advertise the quality of an embedding model. It preserves native macOS arm64 and Linux amd64 builds, the Apache-2.0 Codex plugin, deterministic Markdown migration, source-locked knowledge proposals, encrypted Knowledge Vault recovery, reproducible lexical BEIR gates, and private-by-default storage. Agent-private memory and disposable vectors never enter the shared knowledge backup.

Start with the release archive, verify `SHA256SUMS`, run the installer, create an Agent identity, and connect the stdio MCP server. The architecture and threat model are public so teams can evaluate the boundary rather than trust a slogan.
