#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
art_bin="${ART_BIN:-$repo_root/target/debug/art}"
test_parent="/tmp/art-v0.1.0-release-migration-20260830"
mkdir -p "$test_parent"
test_root="$(mktemp -d "$test_parent/migration-contract.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

source_root="$test_root/source"
art_home="$test_root/art-home"
receipt="$test_root/migration.json"
mkdir -p "$source_root/10-systems" "$source_root/30-operations"
printf '%s\n' '# Alpha System' '' 'ART_ALPHA_MIGRATION_TOKEN' > "$source_root/10-systems/alpha.md"
printf '%s\n' '--dangerous-printf-input' > "$source_root/10-systems/dashes.md"
printf '%s\n' '# 中文恢复流程' '' '通过 [[Alpha System]] 验证 ART_中文迁移_8821。' > "$source_root/30-operations/chinese.md"

"$art_bin" --home "$art_home" init --confirm >/dev/null
"$art_bin" --home "$art_home" agent create --id codex-primary --host codex >/dev/null

if python3 "$repo_root/scripts/migrate_markdown_knowledge.py" \
    --art "$art_bin" --home "$art_home" --source "$source_root" \
    --agent codex-primary --receipt "$receipt" 2>/dev/null; then
  echo 'migration unexpectedly accepted without --confirm-reviewed' >&2
  exit 1
fi

python3 "$repo_root/scripts/migrate_markdown_knowledge.py" \
  --art "$art_bin" --home "$art_home" --source "$source_root" \
  --agent codex-primary --receipt "$receipt" --confirm-reviewed >/dev/null

python3 "$repo_root/scripts/verify_migration_receipt.py" \
  --art "$art_bin" --home "$art_home" --source "$source_root" \
  --receipt "$receipt" >/dev/null

python3 - "$receipt" <<'PY'
import json
import pathlib
import sys

receipt = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
assert receipt["schema"] == "art.knowledge.migration.receipt.v1"
assert receipt["source_file_count"] == 3
assert len(receipt["items"]) == 3
assert all(item["status"] == "published" for item in receipt["items"])
assert len({item["edition_id"] for item in receipt["items"]}) == 3
assert len({item["knowledge_key"] for item in receipt["items"]}) == 3
PY

before="$(shasum -a 256 "$receipt" | awk '{print $1}')"
python3 "$repo_root/scripts/migrate_markdown_knowledge.py" \
  --art "$art_bin" --home "$art_home" --source "$source_root" \
  --agent codex-primary --receipt "$receipt" --confirm-reviewed >/dev/null
after="$(shasum -a 256 "$receipt" | awk '{print $1}')"
test "$before" = "$after"

recall="$($art_bin --home "$art_home" recall 'ART_中文迁移_8821' --agent codex-primary --json)"
python3 - "$recall" <<'PY'
import json
import sys

bundle = json.loads(sys.argv[1])
assert len(bundle["knowledge_editions"]) == 1
assert bundle["knowledge_editions"][0]["title"] == "中文恢复流程"
PY

printf '%s\n' '# changed' > "$source_root/10-systems/alpha.md"
if python3 "$repo_root/scripts/verify_migration_receipt.py" \
    --art "$art_bin" --home "$art_home" --source "$source_root" \
    --receipt "$receipt" 2>/dev/null; then
  echo 'verification unexpectedly accepted a changed source' >&2
  exit 1
fi

echo 'migration contract passed'
