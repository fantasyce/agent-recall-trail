#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
art_bin="${ART_BIN:-target/debug/art}"

cargo metadata --format-version 1 --no-deps \
  | jq -e 'all(.packages[]; .version == "0.3.0")' >/dev/null
test "$($art_bin --version)" = 'art 0.3.0'
jq -e '.name == "agent-recall-trail" and .version == "0.3.0"' \
  plugin/agent-recall-trail/.codex-plugin/plugin.json >/dev/null
rg -q 'four explicit retrieval modes' README.md
rg -q 'lexical.*full_scan.*semantic.*hybrid' README.md
rg -q 'optional embedding' README.md
rg -q 'backup create --output' docs/operations.md
rg -q 'art backup verify --source' docs/operations.md
rg -q 'art backup restore --source' docs/operations.md
rg -q 'never pushes automatically' docs/operations.md
rg -q 'encrypted recovery capsule' docs/security-model.md

echo 'release version contract passed'
