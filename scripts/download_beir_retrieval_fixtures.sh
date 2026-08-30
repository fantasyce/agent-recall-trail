#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
if [[ -z "$target" || "$target" == / || "$target" == "$HOME" || -e "$target" ]]; then
  echo "usage: $0 <new-output-directory>" >&2
  exit 2
fi
mkdir -m 700 "$target"

verify_md5() {
  local file="$1" expected="$2" actual
  if command -v md5 >/dev/null 2>&1; then
    actual="$(md5 -q "$file")"
  else
    actual="$(md5sum "$file" | awk '{print $1}')"
  fi
  [[ "$actual" == "$expected" ]] || {
    echo "checksum mismatch for $file" >&2
    exit 1
  }
}

for dataset in scifact nfcorpus; do
  curl -fL --retry 3 \
    -o "$target/$dataset.zip" \
    "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/$dataset.zip"
done
verify_md5 "$target/scifact.zip" 5f7d1de60b170fc8027bb7898e2efca1
verify_md5 "$target/nfcorpus.zip" a89dba18a62ef92f7d323ec890a0d38d
unzip -q "$target/scifact.zip" -d "$target"
unzip -q "$target/nfcorpus.zip" -d "$target"
echo "BEIR_FIXTURES_READY=$target"
