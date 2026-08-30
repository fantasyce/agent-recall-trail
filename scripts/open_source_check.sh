#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
for f in README.md LICENSE SECURITY.md CONTRIBUTING.md CHANGELOG.md SUPPORT.md THIRD_PARTY_NOTICES.md; do [[ -s "$repo/$f" ]] || { echo "missing public file: $f" >&2; exit 1; }; done
for word in 'Private Recall Trails' 'Knowledge Editions' 'human' 'Codex' 'DSH'; do rg -F -q "$word" "$repo/README.md" "$repo/site/index.html" || exit 1; done
if git -C "$repo" grep -n -E -I '/Users/[^/]+/|BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{20,}' -- . ':(exclude,glob)docs/specs/**' ':(exclude,glob)docs/plans/**' ':(exclude)scripts/open_source_check.sh'; then echo 'private path or credential-like content found' >&2; exit 1; fi
bash "$repo/tests/scripts/independence-scan.sh"; bash "$repo/scripts/test_site.sh"; bash "$repo/scripts/test_launch_surface.sh"; echo 'OPEN_SOURCE_CHECK=PASS'
