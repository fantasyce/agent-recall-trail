#!/usr/bin/env bash
set -euo pipefail

art_bin="${ART_BIN:?ART_BIN is required}"
root="$(mktemp -d "${TMPDIR:-/tmp}/art-backup-e2e.XXXXXX")"
trap 'find "$root" -depth -delete 2>/dev/null || true' EXIT
source_home="$root/source-home"; bare="$root/remote.git"; repo="$root/repo"; clone="$root/clone"; restored="$root/restored"
"$art_bin" --home "$source_home" init --confirm >/dev/null
sqlite3 "$source_home/data/art/knowledge-vault/art-control.sqlite3" "CREATE TABLE backup_marker(value TEXT); INSERT INTO backup_marker VALUES('CONTROL_PLAINTEXT_MUST_NOT_LEAK');"
git init --bare "$bare" >/dev/null
git clone "$bare" "$repo" >/dev/null 2>&1
git -C "$repo" config user.name "ART Backup Test"
git -C "$repo" config user.email "art-backup@example.invalid"
printf '%s\n' '# ART Knowledge Backup' > "$repo/README.md"
git -C "$repo" add README.md && git -C "$repo" commit -m init >/dev/null && git -C "$repo" push origin HEAD:main >/dev/null
git -C "$bare" symbolic-ref HEAD refs/heads/main
age-keygen -o "$root/identity.txt" 2> "$root/recipient.txt"
age-keygen -y "$root/identity.txt" > "$root/recipients.txt"
"$(dirname "$0")/../../scripts/backup_knowledge_to_git.sh" "$art_bin" "$source_home" "$repo" "$root/recipients.txt" "$bare" --confirm >/dev/null
! git -C "$repo" grep -q CONTROL_PLAINTEXT_MUST_NOT_LEAK
git -C "$repo" push origin HEAD:main >/dev/null
git clone "$bare" "$clone" >/dev/null 2>&1
"$(dirname "$0")/../../scripts/restore_knowledge_from_git.sh" "$art_bin" "$clone" "$root/identity.txt" "$restored" --confirm >/dev/null
[[ "$(sqlite3 "$restored/data/art/knowledge-vault/art-control.sqlite3" "SELECT value FROM backup_marker;")" == "CONTROL_PLAINTEXT_MUST_NOT_LEAK" ]]
[[ "$(find "$repo" -type f -not -path "$repo/.git/*" -print | sed "s#^$repo/##" | sort | tr '\n' ' ')" == "README.md art-backup.json recovery/control-and-key.tar.age recovery/recovery-manifest.json " ]]
echo "backup recovery shell E2E passed"
