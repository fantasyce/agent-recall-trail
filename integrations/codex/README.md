# Codex integration

Use an absolute release-binary path and a pre-created Agent ID. Preview the generated block with `art integration codex --agent codex-primary --dry-run`; review before adding it to Codex configuration. ART does not modify Codex internal memory.

Codex may require approval for MCP calls depending on its active approval policy. For isolated automated acceptance only, run Codex in a task-owned directory and explicit no-approval mode. Do not weaken a normal user's policy just to make ART calls silent.
