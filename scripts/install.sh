#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'usage: install.sh --binary PATH [--home PATH] [--link PATH] --confirm' >&2
}

user_root="$(cd && pwd -P)"
art_home="$user_root/.across"
link_path="$user_root/.local/bin/art"
binary=""
confirm=false

while (($#)); do
  case "$1" in
    --binary) binary="${2:?missing --binary value}"; shift 2 ;;
    --home) art_home="${2:?missing --home value}"; shift 2 ;;
    --link) link_path="${2:?missing --link value}"; shift 2 ;;
    --confirm) confirm=true; shift ;;
    *) usage; exit 64 ;;
  esac
done

if [[ "$confirm" != true || -z "$binary" ]]; then
  usage
  exit 64
fi
if [[ "$art_home" != /* || "$link_path" != /* || "$art_home" == / || "$art_home" == "$user_root" ]]; then
  echo 'ART home and link must be narrow absolute paths' >&2
  exit 64
fi
if [[ "$art_home" == *'"'* || "$art_home" == *$'\n'* ]]; then
  echo 'ART home contains unsupported JSON path characters' >&2
  exit 64
fi
binary="$(cd "$(dirname "$binary")" && pwd -P)/$(basename "$binary")"
if [[ ! -f "$binary" || ! -x "$binary" ]]; then
  echo 'ART binary must be a regular executable file' >&2
  exit 64
fi
if [[ "$($binary --version)" != 'art 0.1.1' ]]; then
  echo 'ART installer accepts only the 0.1.1 binary' >&2
  exit 64
fi

mkdir -p "$art_home/bin" "$art_home/config/art" "$art_home/data/art" "$art_home/logs/art"
chmod 700 "$art_home/config/art" "$art_home/data/art" "$art_home/logs/art"
staging="$(mktemp -d "$art_home/.art-install.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
install -m 0755 "$binary" "$staging/art"
test "$("$staging/art" --version)" = 'art 0.1.1'
mv -f "$staging/art" "$art_home/bin/art"

config_tmp="$art_home/config/art/.config.json.tmp.$$"
printf '{\n  "schema": "art.config.v1",\n  "home": "%s"\n}\n' "$art_home" > "$config_tmp"
chmod 600 "$config_tmp"
mv -f "$config_tmp" "$art_home/config/art/config.json"

if [[ ! -f "$art_home/config/art/commitment.key" ]]; then
  "$art_home/bin/art" --home "$art_home" init --confirm >/dev/null
fi

mkdir -p "$(dirname "$link_path")"
if [[ -e "$link_path" && ! -L "$link_path" ]]; then
  echo "refusing to replace non-symbolic-link path: $link_path" >&2
  exit 73
fi
if [[ -L "$link_path" && "$(readlink "$link_path")" != "$art_home/bin/art" ]]; then
  echo "refusing to replace a link owned by another installation: $link_path" >&2
  exit 73
fi
link_tmp="$(dirname "$link_path")/.art-link.$$"
ln -s "$art_home/bin/art" "$link_tmp"
mv -f "$link_tmp" "$link_path"

echo "installed ART 0.1.1 at $art_home/bin/art"
