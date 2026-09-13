# Community launch copy

Status: the original launch article is published in GitHub Discussion #12. The
v0.3.5 release, MCP Registry entry, project site, updated design-partner call,
and DSH official community discussion are live as of 2026-09-13. Other
account-authenticated community channels remain recorded in the launch
manifest.

## Short post

ART v0.3.5 is open source: private Recall Trails for each coding Agent, plus reviewed shared Knowledge Editions. Its local governance workspace provides exact source and revision review with visible decision and publication states. Human governance is the default; persistent delegated governance is default-off and distinctly labeled when enabled. Local Rust runtime for Codex + DSH; no bundled model or cloud account. https://github.com/fantasyce/agent-recall-trail/releases/tag/v0.3.5

## Technical community post

We built ART around a hard boundary: memory is private to one Agent; knowledge
is an immutable, reviewed artifact. It is not a transcript store. ART v0.3.5
adds an exact-revision governance workspace, visible request states, immutable
publication receipts, and persistent default-off delegated governance. Human
governance remains the default, and delegated actions are recorded as
`AgentDelegated`, never Human. The same local stdio MCP runtime supports Codex
and DSH. Feedback on browser compatibility, provenance, isolation, recovery,
retrieval, and governance semantics is welcome.

## Chinese community post

ART v0.3.5 正式发布：每个 Agent 继续拥有物理隔离的私有记忆，只有经过治理的不可变 Knowledge Edition 才能跨 Agent 共享。新版本加入精确版本审核工作台、清晰的请求与发布状态、不可变发布凭据，以及默认关闭的持久委托治理。人工治理仍是默认模式；启用委托后，动作会明确记录为 `AgentDelegated`，不会冒充人工操作。ART 通过本地 stdio MCP 支持 Codex 与 DSH，不捆绑模型或云账户。
