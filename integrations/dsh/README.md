# DSH integration

## Desktop (official Electron application)

Target: locally installed official desktop `0.2.0-rc.2`. Desktop owns
`$DSH_HOME/profiles/desktop` (`~/.dsh/profiles/desktop` by default). It carries
its matching MCP client; do not install an older client or run
`dsh --profile desktop` / `dsh plugin --profile desktop`.

Use the installed ART binary and a DSH-bound Agent. Preview first:

```bash
art integration dsh --desktop --agent dsh-primary --dry-run
art integration dsh --desktop --agent dsh-primary --apply --output ./art-desktop
```

The output directory must be new and its parent must exist. ART creates a
private export containing `art.overlay.yml`, `art-recall/SKILL.md`, and
`art-recall/references/private-memory-value-v1.md`. It does not edit the host,
install a second DSH runtime, or overwrite an existing export. The patch binds
the current executable, ART home and Agent using literal paths, so launching
Desktop from Finder does not require shell environment variables. Regenerate
the export when the executable, home or Agent changes.

1. In Desktop, open **Account menu → Settings → Open configuration file**.
   Confirm the file is the desktop profile's `cordis.patch.yml`.
2. Back up the existing file. Merge the exported patch entries into its YAML
   array. Replace a lone `[]` with the entries; never append a second array
   below `[]`. Preserve unrelated entries and keep only one `art-memory` entry.
3. Copy the complete exported `art-recall` directory into
   `$DSH_HOME/skills/art-recall`. Preserve the `references` directory. Do not
   replace a modified skill without reviewing it.
4. Quit and reopen Desktop at a safe session boundary. Start a new session,
   load `art-recall`, and call `mcp__art__art_health`. Check the expected Agent
   identity before capture or recall. ART storage must remain outside the
   workspace when stronger same-user separation is required.

To remove the integration, remove only its `art-memory` patch entry and its
skill directory, then restart Desktop. Preserve the ART data home. To roll back,
restore your configuration and skill backups and restart at a safe boundary.
Configuration presence is not proof of a successful MCP connection; verify
health again after each change. A missing binary, unknown Agent or startup
failure requires repairing the path/profile and reconnecting, not reading Vault
files through filesystem tools.

For isolated acceptance, launch the same installed App with task-owned
`DSH_HOME` and `--user-data-dir`, and override the `webserver` port to `0` only
in that test profile. The default desktop port can conflict with the formal
instance. Do not use the user's formal home as a test fixture.

## CLI compatibility

Historical host validation used `dsh 0.1.1-rc.2` on 2026-08-30. The existing
non-desktop export remains a patch preview for CLI profiles:

```bash
art integration dsh --agent dsh-primary --print
```

The environment-based sample is explicitly opt-in:

```bash
ART_BINARY=/absolute/path/to/art \
ART_HOME=/task-owned/art-home \
ART_AGENT=dsh-primary \
dsh --profile headless --patch integrations/dsh/art.overlay.yml "your task"
```

Use the application-bundled CLI for compatibility checks when available;
Desktop acceptance still requires a real desktop session.

ART guarantees bounded EOF/signal shutdown and database integrity. DSH owns reconnect attempts and retains tool-call history already delivered to its session. Do not place the ART data root in an Agent workspace when stronger same-user isolation is required.

Open human governance through `mcp__art__art_governance_ui_open` in DSH's page
surface. The same ART-owned page handles persistent delegation settings,
review, publication, status, and audit. Delegation is default-off and bound to
the local host plus Agent identity; when enabled, one unambiguous request uses
`approve_and_publish` and records `AgentDelegated`. Do not route people to a
shell workflow.

DSH supports proactive private memory without a DSH Hook. Ordinary work uses
`mcp__art__art_memory_capture` with `capture_origin=agent_initiated` and a
`value_reason`. This shares the machine-wide automatic switch, budget and
cooldown with Codex Hook intake. The persistent Agent/day fallback applies
without trusted host session attribution. The value standard is
[private-memory value standard v1](../../plugin/agent-recall-trail/skills/agent-recall-trail/references/private-memory-value-v1.md).

Explicit user requests use `capture_origin=user_requested` and a sanitized
`request_basis`; these and recall remain available when automatic memory is
off. The same nine tools apply. DSH must not emulate the Codex Stop Hook or use
`art_memory_candidate_submit` without its genuine trigger receipt. Keep the
value-standard reference available with the integration skill when packaging
it separately. Live proactive/explicit DSH acceptance for this candidate is
reported separately from the historical host validation above.
