#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
for path in \
  docs/launch/launch-article.md docs/launch/faq.md docs/launch/community-posts.md \
  docs/launch/maintainer-outreach.md docs/launch/showcase-submission.md \
  docs/launch/launch-manifest.json packaging/mcp-registry/server.json.in \
  scripts/build_release_binary.sh scripts/build_release_assets.sh scripts/verify_release_assets.sh scripts/open_source_check.sh \
  .github/workflows/quality.yml .github/workflows/release.yml .github/workflows/publish-mcp.yml; do
  [[ -s "$repo_dir/$path" ]] || { echo "missing launch surface: $path" >&2; exit 1; }
done
python3 - "$repo_dir" <<'PY'
import json, pathlib, sys
repo = pathlib.Path(sys.argv[1])
manifest = json.loads((repo / "docs/launch/launch-manifest.json").read_text())
assert manifest["schema_version"] == 1
assert manifest["release"] == "v0.3.0"
channels = {item["id"]: item for item in manifest["channels"]}
assert {"github-release", "mcp-registry", "github-pages", "github-discussion", "design-partners"} <= channels.keys()
for item in channels.values():
    assert item["status"] in {"prepared", "published", "blocked"}
    if item["status"] == "published": assert item["public_url"].startswith("https://")
release = (repo / ".github/workflows/release.yml").read_text()
registry = (repo / ".github/workflows/publish-mcp.yml").read_text()
assert "git rev-parse origin/main" in release
assert "ref: v0.3.0" in registry
assert "login github-oidc" in registry
assert "io.github.fantasyce%2Fagent-recall-trail/versions/0.3.0" in registry
for workflow in (release, registry):
    assert "actions/checkout@v" not in workflow
PY
for phrase in 'not a transcript store' 'human-reviewed' 'private' 'Codex' 'DSH'; do
  rg -F -q "$phrase" "$repo_dir/docs/launch" || { echo "launch copy missing boundary: $phrase" >&2; exit 1; }
done
echo 'launch surface tests passed'
