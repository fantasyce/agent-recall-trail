#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo_root"

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
python3 tests/scripts/test_beir_harness.py
bash tests/scripts/test_migration.sh
cargo test --release -p art-retrieval --test performance_contracts -- --ignored
cargo build --release --locked
ART_BIN=target/release/art bash tests/scripts/test_release_version.sh
ART_BIN=target/release/art bash tests/scripts/test_plugin_surface.sh
ART_BIN=target/release/art bash tests/scripts/test_install_lifecycle.sh
bash scripts/test_site.sh
bash scripts/test_launch_surface.sh
bash scripts/open_source_check.sh
python3 tests/scripts/stress_gate.py target/release/art
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
