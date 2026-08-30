#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'usage: uninstall.sh [--home PATH] [--link PATH] --confirm [--purge-data --confirm-purge]' >&2
}

user_root="$(cd && pwd -P)"
art_home="$user_root/.across"
link_path="$user_root/.local/bin/art"
confirm=false
purge=false
confirm_purge=false

while (($#)); do
  case "$1" in
    --home) art_home="${2:?missing --home value}"; shift 2 ;;
    --link) link_path="${2:?missing --link value}"; shift 2 ;;
    --confirm) confirm=true; shift ;;
    --purge-data) purge=true; shift ;;
    --confirm-purge) confirm_purge=true; shift ;;
    *) usage; exit 64 ;;
  esac
done

if [[ "$confirm" != true ]]; then
  usage
  exit 64
fi
if [[ "$art_home" != /* || "$link_path" != /* || "$art_home" == / || "$art_home" == "$user_root" ]]; then
  echo 'ART home and link must be narrow absolute paths' >&2
  exit 64
fi
if [[ "$purge" == true && "$confirm_purge" != true ]]; then
  echo 'purging ART data requires --confirm-purge' >&2
  exit 64
fi
if [[ -L "$link_path" ]]; then
  if [[ "$(readlink "$link_path")" != "$art_home/bin/art" ]]; then
    echo "refusing to remove a link owned by another installation: $link_path" >&2
    exit 73
  fi
  rm -f "$link_path"
fi
rm -f "$art_home/bin/art"

if [[ "$purge" == true ]]; then
  rm -rf "$art_home/config/art" "$art_home/data/art" "$art_home/logs/art"
fi

echo 'ART executable uninstalled'
