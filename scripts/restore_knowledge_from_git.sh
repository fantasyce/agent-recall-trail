#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 5 || "$5" != "--confirm" ]]; then
  echo "usage: restore_knowledge_from_git.sh ART_BIN GIT_CLONE IDENTITY_FILE TARGET_HOME --confirm" >&2
  exit 2
fi
art_bin="$1"; clone="$2"; identity="$3"; target="$4"
for tool in sqlite3 age python3 shasum tar; do command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }; done
[[ -x "$art_bin" && -d "$clone/.git" && -f "$identity" && ! -e "$target" ]] || { echo "invalid restore input or target exists" >&2; exit 1; }
[[ "$target" == /* && "$target" != / ]] || { echo "restore target must be a narrow absolute path" >&2; exit 1; }
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
target_parent="$(dirname "$target")"
target_name="$(basename "$target")"
mkdir -p "$target_parent"
staging_home="$target_parent/.${target_name}.recovery-staging.$$.$RANDOM"
[[ ! -e "$staging_home" ]] || { echo "restore staging collision" >&2; exit 1; }
trap 'find "$tmp" -depth -delete 2>/dev/null || true; if [[ -n "${staging_home:-}" && -e "$staging_home" ]]; then find "$staging_home" -depth -delete 2>/dev/null || true; fi' EXIT
chmod 700 "$tmp"
age -d -i "$identity" -o "$tmp/capsule.tar" "$clone/recovery/control-and-key.tar.age"
python3 - "$tmp/capsule.tar" "$tmp/capsule" <<'PY'
import os, sys, tarfile
archive, target = sys.argv[1:]
with tarfile.open(archive) as tf:
    members = tf.getmembers()
    if sorted(m.name for m in members) != ["art-control.sqlite3", "commitment.key"]:
        raise SystemExit("unexpected recovery archive inventory")
    if any(not m.isfile() or m.issym() or m.islnk() for m in members):
        raise SystemExit("recovery archive contains a non-regular member")
    if any(m.size > (1024 * 1024 * 1024 if m.name == "art-control.sqlite3" else 32) for m in members):
        raise SystemExit("recovery archive member exceeds its size limit")
    os.mkdir(target, 0o700)
    for m in members:
        src = tf.extractfile(m)
        if src is None:
            raise SystemExit("recovery archive member cannot be read")
        out = os.path.join(target, m.name)
        with open(out, "xb") as f: f.write(src.read())
        os.chmod(out, 0o600)
PY
[[ "$(wc -c < "$tmp/capsule/commitment.key" | tr -d ' ')" == "32" ]] || { echo "invalid recovered commitment key" >&2; exit 1; }
[[ "$(sqlite3 "$tmp/capsule/art-control.sqlite3" 'PRAGMA integrity_check;')" == "ok" ]] || { echo "recovered Control Store is corrupt" >&2; exit 1; }
"$art_bin" backup restore --source "$clone" --target-home "$staging_home" --commitment-key "$tmp/capsule/commitment.key" --confirm >/dev/null
control="$staging_home/data/art/knowledge-vault/art-control.sqlite3"
find "$(dirname "$control")" -maxdepth 1 \( -name 'art-control.sqlite3-wal' -o -name 'art-control.sqlite3-shm' \) -delete
install -m 0600 "$tmp/capsule/art-control.sqlite3" "$control"
python3 - "$control" "$staging_home/data/art/knowledge-vault" <<'PY'
import os, sqlite3, sys
database, vault = sys.argv[1:]
connection = sqlite3.connect(database)
try:
    for table, column, marker, base in [
        ("publish_intents", "target_dir", "/editions/", os.path.join(vault, "editions")),
        ("event_intents", "target_path", "/.art/events/", os.path.join(vault, ".art/events")),
    ]:
        for row_id, state, old_path in connection.execute(f"SELECT id,state,{column} FROM {table}"):
            if state != "committed" or marker not in old_path:
                raise SystemExit("recovered Control Store contains a non-portable intent")
            suffix = old_path.split(marker, 1)[1]
            new_path = os.path.join(base, suffix)
            connection.execute(f"UPDATE {table} SET {column}=? WHERE id=?", (new_path, row_id))
    connection.executescript("DELETE FROM edition_projections; DELETE FROM knowledge_fts; DELETE FROM knowledge_events;")
    connection.commit()
finally:
    connection.close()
PY
"$art_bin" --home "$staging_home" reindex --knowledge >/dev/null
"$art_bin" --home "$staging_home" doctor --json > "$tmp/doctor.json"
python3 - "$tmp/doctor.json" "$clone/art-backup.json" <<'PY'
import json, sys
doctor = json.load(open(sys.argv[1])); backup = json.load(open(sys.argv[2]))
knowledge = doctor.get("knowledge", {})
if doctor.get("status") != "ok":
    raise SystemExit("restored ART Doctor status is not ok")
if knowledge.get("projection_count") != backup["edition_count"]:
    raise SystemExit("restored ART Edition count mismatch")
if knowledge.get("event_files_verified") != backup["event_count"]:
    raise SystemExit("restored ART event count mismatch")
if not knowledge.get("integrity_ok") or not knowledge.get("search_index_aligned") or not knowledge.get("projection_hashes_ok"):
    raise SystemExit("restored ART knowledge diagnostics are degraded")
PY
mv "$staging_home" "$target"
staging_home=""
echo "ART knowledge restore verified"
