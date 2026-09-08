#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
art_bin="${ART_BIN:-$repo_root/target/debug/art}"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/art-generated-artifacts.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

home="$test_root/across"
"$art_bin" --home "$home" init --confirm >/dev/null
"$art_bin" --home "$home" agent create --id codex-primary --host codex >/dev/null
"$art_bin" --home "$home" mcp schema --agent codex-primary >"$test_root/mcp-tools.schema.json"
cmp "$test_root/mcp-tools.schema.json" "$repo_root/docs/artifacts/mcp-tools.schema.json"

schema_sha="$(shasum -a 256 "$repo_root/docs/artifacts/mcp-tools.schema.json" | awk '{print $1}')"
python3 - "$repo_root/docs/artifacts/schema-inventory.json" "$schema_sha" <<'PY'
import json
import pathlib
import sys

inventory = json.loads(pathlib.Path(sys.argv[1]).read_text())
entry = next(item for item in inventory["schemas"] if item["path"] == "docs/artifacts/mcp-tools.schema.json")
assert entry["sha256"] == sys.argv[2], (entry["sha256"], sys.argv[2])
PY
rg -q "^${schema_sha}  docs/artifacts/mcp-tools.schema.json$" "$repo_root/docs/artifacts/checksums.txt"

echo 'generated artifact contract passed'
