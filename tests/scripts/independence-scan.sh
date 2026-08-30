#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo_root"

forbidden_one='memo''rix'
forbidden_two='led''ger'
forbidden_pattern="${forbidden_one}|${forbidden_two}"

if find . -mindepth 1 -not -path './.git/*' -not -path './target/*' -print \
  | rg -i "$forbidden_pattern"; then
  echo "independence scan: prohibited product shadow in a path" >&2
  exit 1
fi

if rg -ni --hidden --glob '!.git/**' --glob '!target/**' "$forbidden_pattern" .; then
  echo "independence scan: prohibited product shadow in content" >&2
  exit 1
fi

if find . -type f -not -path './.git/*' -not -path './target/*' -size +512k -print -quit \
  | rg -q .; then
  echo "independence scan: unexpectedly large source artifact" >&2
  exit 1
fi

if rg -ni --hidden --glob '!.git/**' --glob '!target/**' \
  --glob '!tests/scripts/independence-scan.sh' \
  '(translated from|copied from|mechanical port of)' .; then
  echo "independence scan: source-attribution header requires review" >&2
  exit 1
fi

if rg -n '^source = "git\+' Cargo.lock; then
  echo "independence scan: Git-sourced dependency rejected" >&2
  exit 1
fi

if find . -mindepth 2 -type d -name .git -not -path './target/*' -print -quit | rg -q .; then
  echo "independence scan: nested source checkout rejected" >&2
  exit 1
fi

if cargo metadata --format-version 1 --no-deps --locked \
  | jq -e 'all(.packages[]; (.name | startswith("art-")) and .license == "Apache-2.0")' \
  >/dev/null; then
  :
else
  echo "independence scan: package identity or license rejected" >&2
  exit 1
fi

echo "independence scan: ok"
