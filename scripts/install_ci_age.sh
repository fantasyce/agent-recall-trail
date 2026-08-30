#!/usr/bin/env bash
set -euo pipefail

version="1.3.2"
install_root="${1:?usage: install_ci_age.sh INSTALL_DIR}"
os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"
case "$arch" in
  x86_64) arch="amd64" ;;
  arm64|aarch64) arch="arm64" ;;
  *) echo "unsupported architecture: $arch" >&2; exit 1 ;;
esac
case "$os-$arch" in
  darwin-amd64) expected="1d1e4bc66e1427edad7739ae7616157de0e79db8b6d2a1497d7d9925fb06a539" ;;
  darwin-arm64) expected="e2020b073c44f692685a24d6abc378817eb81ffaaf49fd0531ef8565f767f2f5" ;;
  linux-amd64) expected="cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10" ;;
  linux-arm64) expected="6b8dc4333c53a5a57c9e5834e3a48f92605d7154014cd07269ff3327db5d37f4" ;;
  *) echo "unsupported platform: $os-$arch" >&2; exit 1 ;;
esac

mkdir -p "$install_root"
chmod 700 "$install_root"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/art-age.XXXXXX")"
trap 'find "$tmp" -depth -delete 2>/dev/null || true' EXIT
archive="$tmp/age.tar.gz"
url="https://github.com/FiloSottile/age/releases/download/v${version}/age-v${version}-${os}-${arch}.tar.gz"
curl --fail --location --proto '=https' --tlsv1.2 --output "$archive" "$url"
actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
test "$actual" = "$expected" || { echo "age archive digest mismatch" >&2; exit 1; }
tar -xzf "$archive" -C "$tmp"
install -m 0755 "$tmp/age/age" "$install_root/age"
install -m 0755 "$tmp/age/age-keygen" "$install_root/age-keygen"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$install_root" >> "$GITHUB_PATH"
fi
printf '%s\n' "$install_root"
