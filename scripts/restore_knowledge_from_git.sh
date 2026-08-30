#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 5 || "$5" != "--confirm" ]]; then
  echo "usage: restore_knowledge_from_git.sh ART_BIN GIT_CLONE IDENTITY_FILE TARGET_HOME --confirm" >&2
  exit 2
fi
art_bin="$1"; clone="$2"; identity="$3"; target="$4"
for tool in sqlite3 age python3 shasum tar; do command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }; done
[[ -x "$art_bin" && -d "$clone/.git" && -f "$identity" && ! -e "$target" ]] || { echo "invalid restore input or target exists" >&2; exit 1; }
"$art_bin" backup verify --source "$clone" >/dev/null
python3 - "$clone/art-backup.json" "$clone/recovery/recovery-manifest.json" "$clone/recovery/control-and-key.tar.age" <<'PY'
import hashlib, json, sys
backup, manifest, capsule = sys.argv[1:]
b = json.load(open(backup)); m = json.load(open(manifest))
if set(m) != {"schema", "knowledge_tree_sha256", "capsule_sha256", "recipient_fingerprint_sha256"}:
    raise SystemExit("invalid recovery manifest fields")
if m["schema"] != "art.recovery.capsule.v1":
    raise SystemExit("invalid recovery manifest schema")
if m["knowledge_tree_sha256"] != b["tree_sha256"]:
    raise SystemExit("recovery manifest is bound to a different knowledge tree")
if m["capsule_sha256"] != hashlib.sha256(open(capsule,"rb").read()).hexdigest():
    raise SystemExit("recovery capsule digest mismatch")
if len(m["recipient_fingerprint_sha256"]) != 64:
    raise SystemExit("invalid recipient fingerprint")
PY
tmp="$(mktemp -d "${TMPDIR:-/tmp}/art-restore.XXXXXX")"
trap 'find "$tmp" -depth -delete 2>/dev/null || true' EXIT
chmod 700 "$tmp"
age -d -i "$identity" -o "$tmp/capsule.tar" "$clone/recovery/control-and-key.tar.age"
python3 - "$tmp/capsule.tar" "$tmp/capsule" <<'PY'
import os, stat, sys, tarfile
archive, target = sys.argv[1:]
with tarfile.open(archive) as tf:
    members = tf.getmembers()
    assert sorted(m.name for m in members) == ["art-control.sqlite3", "commitment.key"]
    assert all(m.isfile() and not (m.issym() or m.islnk()) for m in members)
    os.mkdir(target, 0o700)
    for m in members:
        src = tf.extractfile(m); assert src is not None
        out = os.path.join(target, m.name)
        with open(out, "xb") as f: f.write(src.read())
        os.chmod(out, 0o600)
PY
[[ "$(wc -c < "$tmp/capsule/commitment.key" | tr -d ' ')" == "32" ]] || { echo "invalid recovered commitment key" >&2; exit 1; }
[[ "$(sqlite3 "$tmp/capsule/art-control.sqlite3" 'PRAGMA integrity_check;')" == "ok" ]] || { echo "recovered Control Store is corrupt" >&2; exit 1; }
"$art_bin" backup restore --source "$clone" --target-home "$target" --commitment-key "$tmp/capsule/commitment.key" --confirm >/dev/null
control="$target/data/art/knowledge-vault/art-control.sqlite3"
find "$(dirname "$control")" -maxdepth 1 \( -name 'art-control.sqlite3-wal' -o -name 'art-control.sqlite3-shm' \) -delete
install -m 0600 "$tmp/capsule/art-control.sqlite3" "$control"
sqlite3 "$control" "PRAGMA foreign_keys=ON; DELETE FROM edition_projections; DELETE FROM knowledge_fts; DELETE FROM knowledge_events; DELETE FROM publish_intents; DELETE FROM event_intents;"
"$art_bin" --home "$target" reindex --knowledge >/dev/null
"$art_bin" --home "$target" doctor --json >/dev/null
echo "ART knowledge restore verified"
