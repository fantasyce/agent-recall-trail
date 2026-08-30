# Community launch copy

## Short post

ART v0.2.0 is open source: private Recall Trails for each coding Agent, plus human-reviewed shared Knowledge Editions. The new release adds BM25-first retrieval fusion, bounded Top-K, and reproducible BEIR quality gates while preserving encrypted recovery. Local Rust runtime, Codex + DSH, no cloud account, no autonomous publication. https://github.com/fantasyce/agent-recall-trail/releases/tag/v0.2.0

## Technical community post

We built ART around a hard boundary: memory is private to one Agent; knowledge is an immutable, human-reviewed artifact. It is not a transcript store. ART v0.2.0 strengthens SQLite/FTS5 retrieval with BM25-first fusion and explicit result depth, measured through public BEIR gates, while retaining six bounded stdio MCP tools and encrypted recovery. Feedback on retrieval quality, provenance, isolation, recovery, and review semantics is welcome.

## Chinese community post

ART v0.2.0 正式发布：每个 Agent 继续拥有物理隔离的私有记忆，只有经过人工审查的 Knowledge Edition 才能跨 Agent 共享。新版本加入 BM25 优先融合、可控 Top-K 和可复现的 BEIR 检索门禁，同时保留加密容灾；支持 Codex 与 DSH，本地运行、无需云账户，也不允许 Agent 自主发布知识。
