#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
datasets="${1:-}"
output="${2:-}"
if [[ -z "$datasets" || -z "$output" || ! -d "$datasets" || -e "$output" ]]; then
  echo "usage: $0 <beir-fixture-directory> <new-output.json>" >&2
  exit 2
fi

cargo build --manifest-path "$repo/Cargo.toml" -p art-cli --release
python3 "$repo/scripts/benchmark_beir_retrieval.py" \
  --art "$repo/target/release/art" \
  --datasets "$datasets" \
  --output "$output"
