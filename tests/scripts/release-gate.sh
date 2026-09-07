#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo_root"
cargo_target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
release_art="$cargo_target_dir/release/art"

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --test-threads=1
python3 tests/scripts/test_beir_harness.py
ART_BIN="$cargo_target_dir/debug/art" bash tests/scripts/test_migration.sh
cargo test --release -p art-retrieval --test performance_contracts -- --ignored
bash scripts/build_release_binary.sh
ART_BIN="$release_art" bash tests/scripts/test_release_version.sh
ART_BIN="$release_art" bash tests/scripts/test_plugin_surface.sh
ART_BIN="$release_art" bash tests/scripts/test_install_lifecycle.sh
release_dist="$(mktemp -d "${TMPDIR:-/tmp}/art-release-assets.XXXXXX")"
trap 'rm -rf "$release_dist"' EXIT
release_commit="$(git rev-parse HEAD)"
ART_RELEASE_COMMIT="$release_commit" bash scripts/build_release_assets.sh "$release_dist" darwin_arm64 "$release_art"
ART_RELEASE_COMMIT="$release_commit" bash scripts/build_release_assets.sh "$release_dist" linux_amd64 "$release_art"
bash scripts/verify_release_assets.sh "$release_dist" 0.3.3 "$release_commit"
bash scripts/test_site.sh
bash scripts/test_launch_surface.sh
bash scripts/open_source_check.sh
python3 tests/scripts/stress_gate.py "$release_art"
cargo audit --deny warnings
cargo metadata --format-version 1 --locked \
  | jq -e 'all(.packages[]; (.license != null) and (.license | test("(^|[^A-Z])(AGPL|GPL|SSPL)(-|$)") | not))' \
  >/dev/null
bash tests/scripts/independence-scan.sh

if rg -n --hidden --glob '!.git/**' --glob '!target/**' --glob '!tests/scripts/release-gate.sh' '(BEGIN (RSA|OPENSSH|EC) PRIVATE KEY|Authorization:[[:space:]]*Bearer|api[_-]?key[[:space:]]*=[[:space:]]*[^[:space:]]+)' .; then
  echo "secret scan: rejected" >&2
  exit 1
fi

echo "release gate: ok"
