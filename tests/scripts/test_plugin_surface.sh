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
assert manifest["version"] == "0.3.2"
assert manifest["skills"] == "./skills/"
assert manifest["mcpServers"] == "./.mcp.json"
assert manifest["interface"]["displayName"] == "Agent Recall Trail"
mcp = json.loads((root / ".mcp.json").read_text())["mcpServers"]["agent-recall-trail"]
assert mcp["args"][1:] == ["mcp", "serve", "--agent", "codex-primary"]
skill = (root / "skills/agent-recall-trail/SKILL.md").read_text()
assert skill.startswith("---\nname: agent-recall-trail\n")
assert "Basic Memory" not in skill
assert "approve" in skill and "publish" in skill
assert "full_scan" in skill and "semantic" in skill and "hybrid" in skill
assert "optional" in skill
assert "art_knowledge_governance" in skill
assert "Elicitation" in skill
assert "ordinary chat" in skill
assert "CLI" in skill and "fallback" in skill
policy = (root / "skills/agent-recall-trail/agents/openai.yaml").read_text()
assert "allow_implicit_invocation: true" in policy
bundle_manifest = json.loads((root.parents[1] / "packaging/mcpb/manifest.json.in").read_text())
assert len(bundle_manifest["tools"]) == 7
assert "art_knowledge_governance" in {tool["name"] for tool in bundle_manifest["tools"]}
PY

python3 "$repo_root/tests/scripts/test_plugin_launch.py"
cargo test -p art-mcp --test mcp_contracts tool_surface_is_exactly_seven_agent_safe_tools
echo 'plugin surface contract passed'
