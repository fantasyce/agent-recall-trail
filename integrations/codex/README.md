# Codex integration

Use an absolute release-binary path and a pre-created Agent ID. Preview the generated block with `art integration codex --agent codex-primary --dry-run`; review before adding it to Codex configuration. ART does not modify Codex internal memory.

The packaged Codex plugin starts ART through a small POSIX launcher. It uses
the host's `art` command when available, then the installer's default
`$HOME/.local/bin/art` link. This also supports GUI and restricted SSH hosts
whose noninteractive PATH omits user-local executables. It does not source a
login shell, initialize a Vault, or change Agent identity. Custom installations
outside these locations need an absolute MCP command path.

Installing the skill alone supplies instructions; installing and enabling the
plugin also registers its MCP server and two opt-in automatic-memory hooks.
Codex still presents its normal hook-trust flow; installation alone does not
grant trust. Verify the actual host connection and nine-tool discovery before claiming integration works. A successful standalone
`art` command does not prove that a running Codex task has connected. After
changing plugin files, refresh the host's plugin/skill discovery cache before
reloading its MCP configuration; an MCP reload alone can reuse the old plugin
command. Verify the connection on the next active turn or a fresh session.

Codex may require approval for MCP calls depending on its active approval policy. For isolated automated acceptance only, run Codex in a task-owned directory and explicit no-approval mode. Do not weaken a normal user's policy just to make ART calls silent.

ART exposes a loopback governance page through `art_governance_ui_open`.
Codex opens that URL in its in-app browser for human settings, review,
publication, status, and audit. Delegated governance is persistent and
default-off per host binding and Agent. When enabled, one unambiguous user
instruction can use `approve_and_publish`; ART records `AgentDelegated`.
Full Access alone never enables this policy. Native MCP form Elicitation remains
compatible for other clients, but Codex interaction is page-first and has no
human-facing shell fallback.

The governance Settings view also owns the separate machine-wide automatic
memory switch. It defaults off and covers both ordinary Agent proactive capture
and Hook-triggered capture with shared budget/cooldown. Ordinary work does not
need a Hook: use `art_memory_capture`, `capture_origin=agent_initiated`, and a
bounded `value_reason`. Explicit requests use `capture_origin=user_requested`
and sanitized `request_basis` and remain available while automatic memory is off.

Both paths use [private-memory value standard v1](../../plugin/agent-recall-trail/skills/agent-recall-trail/references/private-memory-value-v1.md).
The Hook supplies this same text during at most one bounded continuation; its
keywords are only a reminder filter. It does not read transcripts, store prompt
text, or publish shared knowledge. Without trusted host session identity,
ordinary MCP calls use the persistent Agent/day fallback. Keep the reference
available with the integration skill when packaging it separately.
