#!/usr/bin/env bash
set -euo pipefail
[[ $# -ge 2 && $# -le 3 ]] || { echo 'usage: build_release_assets.sh DIST darwin_arm64|linux_amd64 [BINARY]' >&2; exit 64; }
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"; repo="$(cd "$script_dir/.." && pwd -P)"; dist="$1"; target="$2"; binary="${3:-$repo/target/release/art}"
case "$target" in darwin_arm64|linux_amd64) ;; *) echo "unsupported target: $target" >&2; exit 65;; esac
version="$(sed -n 's/^version = "\([0-9.]*\)"/\1/p' "$repo/Cargo.toml" | head -1)"; commit="${ART_RELEASE_COMMIT:-$(git -C "$repo" rev-parse HEAD)}"
if [[ ! -x "$binary" ]]; then "$script_dir/build_release_binary.sh"; fi
python3 "$script_dir/build_release_assets.py" --repo "$repo" --dist "$dist" --version "$version" --commit "$commit" --target "$target" --binary "$binary"
