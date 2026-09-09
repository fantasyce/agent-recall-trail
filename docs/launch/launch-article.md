# ART v0.3.5: exact human review for governed Agent knowledge

Status: prepared for review on 2026-09-09.

Coding Agents need continuity, but private experience and shared knowledge do
not have the same authority. ART gives each Agent a physically separate private
Recall Trail, then allows stable material to cross the boundary only as a
governed, immutable Knowledge Edition.

ART is not a transcript store, prompt injector, cloud memory service, or
unbounded autonomous publisher. It is a local Rust runtime for Codex and DSH
with eight bounded MCP tools. Human settings, review, publication, status, and
audit use an ART-owned page opened inside the host. Persistent Agent delegation
is default-off; when the user enables it, one unambiguous instruction can
approve and publish atomically with a distinct `AgentDelegated` audit trail.

The v0.3.5 release turns the loopback governance queue into a complete review
workspace. Reviewers inspect the exact proposal revision, safe rendered and raw
Markdown, locked source provenance, ordered decision history, and a verified
current-Edition comparison before any decision. Approvals, change requests,
rejections, and immutable publication are conflict-safe and return authoritative
receipts. The release retains progressive `route -> recall -> read` retrieval,
four explicit recall modes, and the exact source-anchor vocabulary.

ART does not bundle, choose, train, or advertise the quality of an embedding model. It preserves native macOS arm64 and Linux amd64 builds, the Apache-2.0 Codex plugin, deterministic Markdown migration, source-locked knowledge proposals, encrypted Knowledge Vault recovery, reproducible lexical BEIR gates, and private-by-default storage. Agent-private memory and disposable vectors never enter the shared knowledge backup.

Start with the release archive, verify `SHA256SUMS`, run the installer, create an Agent identity, and connect the stdio MCP server. The architecture and threat model are public so teams can evaluate the boundary rather than trust a slogan.
