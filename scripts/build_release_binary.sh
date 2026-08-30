#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo="$(cd "$script_dir/.." && pwd -P)"
cargo_home="${CARGO_HOME:-${HOME:?HOME is required}/.cargo}"
remap_flags="--remap-path-prefix=$repo=/workspace --remap-path-prefix=$cargo_home=/cargo"

if [[ -n "${RUSTFLAGS:-}" ]]; then
  export RUSTFLAGS="$RUSTFLAGS $remap_flags"
else
  export RUSTFLAGS="$remap_flags"
fi

cargo build --release --locked --manifest-path "$repo/Cargo.toml" -p art-cli
