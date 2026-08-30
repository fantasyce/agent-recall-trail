#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { echo 'usage: install_ci_ripgrep.sh OUTPUT_DIRECTORY' >&2; exit 64; }
output="$1"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset='ripgrep-14.1.1-aarch64-apple-darwin.tar.gz' ;;
  Darwin-x86_64) asset='ripgrep-14.1.1-x86_64-apple-darwin.tar.gz' ;;
  Linux-x86_64) asset='ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz' ;;
  *) echo "unsupported CI host: $(uname -s)-$(uname -m)" >&2; exit 65 ;;
esac
base='https://github.com/BurntSushi/ripgrep/releases/download/14.1.1'
curl --fail --location --proto '=https' --tlsv1.2 "$base/$asset" --output "$output/$asset"
curl --fail --location --proto '=https' --tlsv1.2 "$base/$asset.sha256" --output "$output/$asset.sha256"
expected="$(awk 'NR == 1 {print $1}' "$output/$asset.sha256")"
actual="$(shasum -a 256 "$output/$asset" | awk '{print $1}')"
[[ "$expected" =~ ^[a-f0-9]{64}$ && "$actual" == "$expected" ]] || {
  echo 'ripgrep archive checksum mismatch' >&2
  exit 1
}
tar -xzf "$output/$asset" -C "$output"
root="${asset%.tar.gz}"
install -m 0755 "$output/$root/rg" "$output/rg"
"$output/rg" --version | grep -Fq 'ripgrep 14.1.1'
printf '%s\n' "$output"
