#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
art_bin="${ART_BIN:-target/debug/art}"

cargo metadata --format-version 1 --no-deps \
  | jq -e 'all(.packages[]; .version == "0.1.0")' >/dev/null
test "$($art_bin --version)" = 'art 0.1.0'
jq -e '.name == "agent-recall-trail" and .version == "0.1.0"' \
  plugin/agent-recall-trail/.codex-plugin/plugin.json >/dev/null

echo 'release version contract passed'
