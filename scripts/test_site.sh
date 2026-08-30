#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
index="$repo_dir/site/index.html"

for path in site/index.html site/styles.css site/art-provenance-trail.svg site/404.html .github/workflows/pages.yml; do
  [[ -s "$repo_dir/$path" ]] || { echo "missing site surface: $path" >&2; exit 1; }
done
[[ "$(grep -Eoc '<h1([ >])' "$index")" -eq 1 ]]
for landmark in header main footer nav; do grep -Eq "<$landmark([ >])" "$index"; done
for phrase in 'Your Agent remembers. Shared knowledge is reviewed.' 'Private Recall Trails' 'Knowledge Editions' 'Codex' 'DSH' 'releases/latest' 'SECURITY.md'; do
  grep -Fq "$phrase" "$index" || { echo "missing site phrase: $phrase" >&2; exit 1; }
done
grep -Fq '<title' "$repo_dir/site/art-provenance-trail.svg"
grep -Fq '<desc' "$repo_dir/site/art-provenance-trail.svg"
for pin in \
  'actions/configure-pages@983d7736d9b0ae728b81ab479565c72886d7745b' \
  'actions/upload-pages-artifact@7b1f4a764d45c48632c6b24a0339c27f5614fb0b' \
  'actions/deploy-pages@d6db90164ac5ed86f2b6aed7e0febac5b3c0c03e'; do
  grep -Fq "$pin" "$repo_dir/.github/workflows/pages.yml"
done
if rg -n 'https?://[^" ]+\.(js|css|woff2?|ttf)|<script|googletag|analytics|href="#"|TODO|PLACEHOLDER' "$repo_dir/site"; then
  echo 'site contains an external dependency, tracker, script, or placeholder' >&2
  exit 1
fi
echo 'site tests passed'
