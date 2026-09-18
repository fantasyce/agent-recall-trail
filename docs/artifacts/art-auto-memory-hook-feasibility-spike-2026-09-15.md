# ART automatic memory through Codex hooks: feasibility spike

Date: 2026-09-15

Status: **GO for a bounded implementation phase; NOT ready for default-on release**

## Question

Can the ART Codex plugin add selective automatic memory capture without a
Codex App Server change, and without adding model cost to every turn?

## Scope and isolation

- ART binary: 0.3.5
- Codex CLI: 0.154.0
- ART source commit: `b86ad22e6be2722f5f3cb6e4661dcafd30530192`
- A task-owned Codex home, marketplace, plugin cache, ART home, Agent identity,
  workspace, and evidence directory were used.
- The formal ART plugin, Codex configuration, Agent identity, and Vault were not
  used for spike writes.
- Receipts stored only field presence, lengths, hashes, status codes, and
  timing. They did not copy prompts, assistant messages, or transcripts.

The temporary plugin bundled `SessionStart`, `Stop`, and `SessionEnd`
command hooks plus a `Stop` MCP-tool hook. A transparent stdio proxy recorded
only MCP method names, tool names, and success/error metadata.

## Results

### Plugin discovery and trust

- The plugin passed the Codex plugin validator.
- Codex installed it from a task-owned marketplace and cached the hook and MCP
  payload with matching SHA-256 hashes.
- With `--dangerously-bypass-hook-trust` for this isolated automation, all
  configured hooks ran.
- Without that one-off bypass and without persisted trust, command-hook receipt
  count did not change. The ordinary Agent turn still completed.

Conclusion: plugin-bundled hooks are discoverable, but production onboarding
must explicitly surface Codex hook review/trust. Installing or enabling the
plugin alone is not proof that hooks will run.

### Lifecycle inputs

One real non-ephemeral session produced all three expected command events:

- `SessionStart`
- `Stop`
- `SessionEnd`

The real `Stop` input included:

- a stable session identifier;
- a stable turn identifier;
- `stop_hook_active=false`;
- the final assistant message;
- model, permission mode, and working directory.

The non-ephemeral session exposed a transcript path that existed and was
readable at `SessionStart`, `Stop`, and `SessionEnd`. The final receipt
observed 36,638 bytes. An ephemeral `codex exec --ephemeral` session exposed
no transcript path.

Conclusion: `last_assistant_message` is the stable minimum input. Transcript
reading must be optional because ephemeral sessions omit it and the documented
transcript format is not stable.

### ART MCP reachability

The stdio metadata receipts captured this successful sequence:

1. `initialize`
2. `notifications/initialized`
3. `tools/list`
4. `tools/call` for `art_health`
5. a matching ART result with no JSON-RPC or tool-level error

Conclusion: a `Stop` hook can synchronously invoke an already connected ART
MCP tool. No Codex App Server change is required for this transport path.

### Isolated ART write, recall, exact read, and idempotency

The first capture attempt deliberately preserved its failure evidence. It used
an invalid `scope_type=project` and ART returned `ART_INVALID_INPUT`; the
isolated Vault remained at zero memories. Source tracing showed that ART 0.3.5
accepts `session`, `repository`, `workspace`, `machine`, or `user`.

Changing only the scope type to `repository` produced:

- MCP `art_memory_capture` result with `isError=false`;
- one Active memory, one revision, and one anchor;
- successful lexical recall;
- successful exact revision read;
- the same memory count after replaying the identical idempotency key.

A second test expanded `${last_assistant_message}` and `${turn_id}` inside
the MCP input. The synthetic final message `DYNAMIC_MEMORY_SOURCE` was stored
exactly and immediately recalled. The isolated Vault then contained two
memories, two revisions, and two anchors.

Conclusion: both static structured capture and dynamic per-turn data binding
work. Current `art_memory_capture` makes a sourced capture Active immediately,
so directly mapping every final message into this tool would pollute recall.

### Exactly-once continuation

A `Stop` command hook returned a continuation decision only when
`stop_hook_active=false`.

Observed sequence for the same session and turn identifiers:

1. Agent output: `ORIGINAL_DONE`
2. `Stop` receipt: `stop_hook_active=false`
3. Hook-created continuation prompt
4. Agent output: `HOOK_CONTINUED`
5. `Stop` receipt: `stop_hook_active=true`
6. Normal completion, with no third pass

Conclusion: a selective second-pass memory decision can be implemented without
an infinite loop, but it has material token cost.

### Time and token comparison

Three samples used the same model, prompt shape, sandbox, ART MCP, and plugin.
The baseline disabled only the hooks feature. The enabled case ran lightweight
command hooks and a synchronous `art_health` MCP hook.

| Mode | Wall time samples (s) | Mean (s) | Input tokens | Output tokens |
| --- | --- | ---: | --- | --- |
| hooks disabled | 9.910, 12.611, 14.243 | 12.255 | 12,249 each | 7 each |
| hooks enabled | 13.987, 15.421, 15.819 | 15.076 | 12,249 each | 7 each |

The observed mean delta was +2.821 seconds. This is a small, network-sensitive
sample and is not a production latency benchmark. It does show that
informational command/MCP hooks added no model tokens in these runs.

The exactly-once continuation used 24,577 input tokens, 27 output tokens, and
10 reasoning tokens, compared with 12,249 input and 7 output tokens for the
single-pass run.

Conclusion: passive hooks need not add model tokens, but forcing a model
continuation roughly doubled input tokens in this probe. It must be selective,
not per-turn by default.

### Failure behavior and privacy

- With a failing command hook and a missing MCP server, the main turn still
  returned the requested result with exit code zero.
- The Codex JSON event stream did not provide a sufficiently explicit
  user-facing capture failure.
- Hook and MCP receipt files were mode `0600`.
- Searches for every synthetic prompt and assistant-output marker found none in
  the hook/MCP receipts.
- The spike proxy logged tool names and error codes only; it did not log tool
  arguments, tool content, or transcript content.

Conclusion: fail-open behavior protects the coding workflow, but ART must own a
visible capture status, bounded retry receipt, and diagnostics. Silent loss is
not acceptable.

## Recommended product shape

Do not ship “save every turn as Active memory.”

Implement a two-tier flow:

1. A lightweight `Stop` command hook performs a deterministic eligibility
   gate. Normal turns return immediately and make no model call.
2. Explicit “remember this” signals and high-confidence durable outcomes may
   trigger exactly one continuation. The continuation asks the current Agent to
   create a bounded, sourced ART memory and relies on `stop_hook_active` to
   prevent loops.
3. Add an ART auto-capture candidate/quarantine contract before enabling
   broader automatic extraction. Automatically extracted content must not
   become recallable Active memory until it passes quality, sensitivity,
   duplication, and conflict checks.
4. Use a per-turn idempotency key derived from session id, turn id, and a
   content hash. Never use one static key in production.
5. Treat transcript access as optional. Never copy a raw transcript into ART;
   support final-message-only operation and fail closed on unknown transcript
   formats.
6. Keep capture failure non-blocking, but persist a bounded local receipt and
   show retryable status to the user.
7. Make automatic capture opt-in initially, with a configurable threshold,
   per-session budget, and an immediate off switch.

## Implementation acceptance gates

- Hook trust onboarding works in the Codex app and CLI without a bypass flag.
- Non-triggered `Stop` overhead has a defined p95 target and adds zero model
  tokens.
- Trigger frequency and token budget are measured on representative sessions.
- The same turn cannot create duplicates across retries, resume, or restart.
- Auto-extracted content lands in Candidate/quarantine, not Active recall.
- Secrets, raw transcripts, and unrestricted tool output are rejected.
- Ephemeral sessions, absent transcripts, hook timeout, ART unavailable, and
  process interruption all have tests.
- Capture failure is visible and retryable without blocking the main turn.
- Formal plugin packaging, clean install, update, rollback, and uninstall tests
  cover the hook payload and trust state.

## Decision

The plugin-only mechanism is technically feasible. Proceed to a bounded design
and implementation phase for selective, opt-in capture. Do not enable
per-turn model continuation or direct Active-memory capture by default.

Reference: [OpenAI Codex Hooks](https://learn.chatgpt.com/docs/hooks)
