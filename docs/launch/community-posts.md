# Community launch copy

Status: published to GitHub Discussion #12 on 2026-08-31. GitHub Release, MCP Registry, project site, and the public design-partner call are also live; other account-authenticated community channels remain recorded in the launch manifest.

## Short post

ART v0.3.0 is open source: private Recall Trails for each coding Agent, plus human-reviewed shared Knowledge Editions. It adds progressive route/recall/read and user-selected lexical, full-scan, optional semantic, or hybrid retrieval. Local Rust runtime for Codex + DSH; no bundled model, cloud account, or autonomous publication. https://github.com/fantasyce/agent-recall-trail/releases/tag/v0.3.0

## Technical community post

We built ART around a hard boundary: memory is private to one Agent; knowledge is an immutable, human-reviewed artifact. It is not a transcript store. ART v0.3.0 adds bounded navigation maps and four retrieval modes behind one API. Lexical remains the stable default; governed full scan needs no model; semantic and hybrid use an optional provider-neutral embedding endpoint and fall back safely. Feedback on retrieval policy, provenance, isolation, recovery, and review semantics is welcome.

## Chinese community post

ART v0.3.0 正式发布：每个 Agent 继续拥有物理隔离的私有记忆，只有人工审查后的 Knowledge Edition 才能跨 Agent 共享。新版本加入渐进式 route/recall/read，以及可由用户选择的词法、全量扫描、可选语义和混合检索。ART 不捆绑模型；未配置或服务不可用时仍稳定使用词法检索。支持 Codex 与 DSH，本地运行，也不允许 Agent 自主发布知识。
