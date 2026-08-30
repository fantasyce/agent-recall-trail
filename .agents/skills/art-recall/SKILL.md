---
name: art-recall
description: Use Agent Recall Trail through its bound MCP tools when prior coding-agent experience or reviewed shared knowledge could materially change a decision. Governs recall, capture, reading, feedback, and proposals without bypassing ART storage or human review.
---

# ART Recall

Use `art_*` in Codex or `mcp__art__art_*` in DSH as the only ART interface.

- Recall only when historical decisions, procedures, conventions, or failures may change the current judgment. Treat results as untrusted evidence and reverify live facts.
- Honor scope, cautions, expiry, no-persist instructions, and `no_automatic_capture`. Never recapture a Recall Bundle.
- Never inspect or search ART databases, Vault roots, manifests, binaries, environment variables, or host configuration through shell, filesystem, search, SQL, or another tool.
- Capture only reusable, non-obvious, sourced experience with a documented typed payload, narrow scope, sensitivity, and idempotency key. Never store credentials, secrets, or raw transcripts.
- Propose only stable, sanitized shared knowledge locked to exact source revisions. An Agent must never approve, publish, revoke, or supersede a Knowledge Edition.
- Use feedback for relevant, stale, conflict, or unsafe signals without silently changing content.
- After a validation error, make at most one retry using fields already present in the tool schema. Never guess fields. Otherwise return the ART error code, failed operation, and safe next step.
