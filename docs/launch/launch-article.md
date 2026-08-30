# ART v0.2.0: stronger local recall without weakening memory boundaries

Coding Agents need continuity, but a shared transcript bucket is the wrong authority. ART gives each Agent a physically separate private Recall Trail, then makes reusable knowledge cross the boundary only through a human-reviewed, immutable Knowledge Edition.

ART is not a transcript store, prompt injector, cloud memory service, or autonomous publisher. It is a local Rust runtime for Codex and DSH with six bounded MCP tools. Agents can recall, capture, read, give feedback, and draft proposals; only a human operator can approve or publish shared knowledge.

The v0.2.0 release adds BM25-ranked broad candidate retrieval, bounded result depth across CLI and MCP, and full-split BEIR SciFact and NFCorpus product-path gates. It preserves native macOS arm64 and Linux amd64 builds, the Apache-2.0 Codex plugin, deterministic Markdown migration, source-locked knowledge proposals, encrypted Knowledge Vault recovery, and a private-by-default SQLite/FTS5 storage model. Agent-private memory never enters the shared knowledge backup.

Start with the release archive, verify `SHA256SUMS`, run the installer, create an Agent identity, and connect the stdio MCP server. The architecture and threat model are public so teams can evaluate the boundary rather than trust a slogan.
