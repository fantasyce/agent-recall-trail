#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 6 || "$6" != "--confirm" ]]; then
  echo "usage: backup_knowledge_to_git.sh ART_BIN ART_HOME GIT_WORKTREE RECIPIENT_FILE EXPECTED_REPOSITORY --confirm" >&2
  exit 2
fi
art_bin="$1"; art_home="$2"; repo="$3"; recipient="$4"; expected_repo="$5"
for tool in git sqlite3 age python3 shasum tar; do command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }; done
[[ -x "$art_bin" && -d "$art_home" && -d "$repo/.git" && -f "$recipient" ]] || { echo "invalid backup input" >&2; exit 1; }
[[ -z "$(git -C "$repo" status --porcelain)" ]] || { echo "Git worktree must be clean" >&2; exit 1; }
for marker in MERGE_HEAD rebase-apply rebase-merge CHERRY_PICK_HEAD; do [[ ! -e "$(git -C "$repo" rev-parse --git-path "$marker")" ]] || { echo "Git operation in progress" >&2; exit 1; }; done
remote="$(git -C "$repo" remote get-url origin)"
if [[ "$expected_repo" == /* ]]; then
  [[ "$remote" == "$expected_repo" ]] || { echo "unexpected Git repository" >&2; exit 1; }
else
  normalized="${remote%.git}"; normalized="${normalized#git@github.com:}"; normalized="${normalized#https://github.com/}"
  [[ "$normalized" == "$expected_repo" ]] || { echo "unexpected GitHub repository" >&2; exit 1; }
  command -v gh >/dev/null || { echo "gh is required for GitHub privacy verification" >&2; exit 1; }
  [[ "$(gh repo view "$expected_repo" --json visibility --jq .visibility)" == "PRIVATE" ]] || { echo "backup repository must be private" >&2; exit 1; }
fi
while IFS= read -r tracked; do
  case "$tracked" in README.md|art-backup.json|knowledge/editions/*|knowledge/events/*|recovery/recovery-manifest.json|recovery/control-and-key.tar.age) ;;
    *) echo "unexpected tracked path: $tracked" >&2; exit 1 ;;
  esac
done < <(git -C "$repo" ls-files)

tmp="$(mktemp -d "${TMPDIR:-/tmp}/art-backup.XXXXXX")"
trap 'find "$tmp" -depth -delete 2>/dev/null || true' EXIT
chmod 700 "$tmp"
snapshot="$tmp/snapshot"
"$art_bin" --home "$art_home" backup create --output "$snapshot" >/dev/null
"$art_bin" backup verify --source "$snapshot" >/dev/null
control="$art_home/data/art/knowledge-vault/art-control.sqlite3"
key="$art_home/config/art/commitment.key"
[[ -f "$control" && -f "$key" ]] || { echo "ART control authority is incomplete" >&2; exit 1; }
pending="$(sqlite3 "$control" "SELECT (SELECT COUNT(*) FROM publish_intents WHERE state!='committed') + (SELECT COUNT(*) FROM event_intents WHERE state!='committed');")"
[[ "$pending" == "0" ]] || { echo "pending ART intents block backup" >&2; exit 1; }
mkdir "$tmp/capsule"; chmod 700 "$tmp/capsule"
sqlite3 "$control" ".backup '$tmp/capsule/art-control.sqlite3'"
install -m 0600 "$key" "$tmp/capsule/commitment.key"
mkdir -p "$repo/recovery"; chmod 700 "$repo/recovery"
COPYFILE_DISABLE=1 tar -C "$tmp/capsule" -cf - art-control.sqlite3 commitment.key | age -R "$recipient" -o "$repo/recovery/control-and-key.tar.age"
chmod 600 "$repo/recovery/control-and-key.tar.age"

find "$repo/knowledge" -depth -delete 2>/dev/null || true
rm_manifest="$repo/art-backup.json"
[[ ! -e "$rm_manifest" ]] || rm "$rm_manifest"
cp -R "$snapshot/knowledge" "$repo/knowledge"
install -m 0600 "$snapshot/art-backup.json" "$repo/art-backup.json"
tree_hash="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["tree_sha256"])' "$repo/art-backup.json")"
capsule_hash="$(shasum -a 256 "$repo/recovery/control-and-key.tar.age" | awk '{print $1}')"
recipient_hash="$(shasum -a 256 "$recipient" | awk '{print $1}')"
python3 - "$repo/recovery/recovery-manifest.json" "$tree_hash" "$capsule_hash" "$recipient_hash" <<'PY'
import json, sys
path, tree, capsule, recipient = sys.argv[1:]
with open(path, "w", encoding="utf-8") as f:
    json.dump({"schema":"art.recovery.capsule.v1","knowledge_tree_sha256":tree,"capsule_sha256":capsule,"recipient_fingerprint_sha256":recipient}, f, sort_keys=True, indent=2)
    f.write("\n")
PY
chmod 600 "$repo/recovery/recovery-manifest.json"
[[ -e "$repo/README.md" ]] || printf '%s\n' '# ART Knowledge Backup' 'Private, verified ART Knowledge Vault disaster-recovery repository.' > "$repo/README.md"
"$art_bin" backup verify --source "$repo" >/dev/null
git -C "$repo" add -- README.md art-backup.json knowledge recovery/recovery-manifest.json recovery/control-and-key.tar.age
git -C "$repo" diff --cached --quiet && { echo "ART backup is already current"; exit 0; }
git -C "$repo" commit -m "backup: ART knowledge $tree_hash" >/dev/null
echo "backup committed locally; explicit git push is still required"
