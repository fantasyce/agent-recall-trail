#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
art_bin="${ART_BIN:-$repo_root/target/debug/art}"
test_parent="/tmp/art-v0.1.0-release-migration-20260830"
mkdir -p "$test_parent"
test_root="$(mktemp -d "$test_parent/install-contract.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
home="$test_root/across"
link="$test_root/local-bin/art"

bash "$repo_root/scripts/install.sh" --binary "$art_bin" --home "$home" --link "$link" --confirm
test -x "$home/bin/art"
test -L "$link"
test "$($link --version)" = 'art 0.1.0'
python3 - "$home/config/art/config.json" "$home" <<'PY'
import json
import pathlib
import sys

config = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert config == {"schema": "art.config.v1", "home": sys.argv[2]}
PY

"$link" --home "$home" agent create --id codex-primary --host codex >/dev/null
test -f "$home/data/art/agents/codex-primary/art.sqlite3"
bash "$repo_root/scripts/install.sh" --binary "$art_bin" --home "$home" --link "$link" --confirm

bash "$repo_root/scripts/uninstall.sh" --home "$home" --link "$link" --confirm
test ! -e "$home/bin/art"
test ! -L "$link"
test -f "$home/data/art/agents/codex-primary/art.sqlite3"

bash "$repo_root/scripts/install.sh" --binary "$art_bin" --home "$home" --link "$link" --confirm
bash "$repo_root/scripts/uninstall.sh" --home "$home" --link "$link" --confirm --purge-data --confirm-purge
test ! -e "$home/bin/art"
test ! -e "$home/config/art"
test ! -e "$home/data/art"
test ! -e "$home/logs/art"

echo 'install lifecycle contract passed'
