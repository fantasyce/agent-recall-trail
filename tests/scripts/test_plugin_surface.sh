#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
plugin="$repo_root/plugin/agent-recall-trail"

python3 - "$plugin" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
manifest = json.loads((root / ".codex-plugin/plugin.json").read_text())
assert manifest["name"] == "agent-recall-trail"
assert manifest["version"] == "0.2.0"
assert manifest["skills"] == "./skills/"
assert manifest["mcpServers"] == "./.mcp.json"
assert manifest["interface"]["displayName"] == "Agent Recall Trail"
mcp = json.loads((root / ".mcp.json").read_text())["mcpServers"]["agent-recall-trail"]
assert mcp == {"command": "art", "args": ["mcp", "serve", "--agent", "codex-primary"]}
skill = (root / "skills/agent-recall-trail/SKILL.md").read_text()
assert skill.startswith("---\nname: agent-recall-trail\n")
assert "Basic Memory" not in skill
assert "approve" in skill and "publish" in skill
policy = (root / "skills/agent-recall-trail/agents/openai.yaml").read_text()
assert "allow_implicit_invocation: true" in policy
PY

cargo test -p art-mcp --test mcp_contracts tool_surface_is_exactly_six_agent_safe_tools
echo 'plugin surface contract passed'
