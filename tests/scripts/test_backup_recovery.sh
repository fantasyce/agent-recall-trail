#!/usr/bin/env bash
set -euo pipefail

art_bin="${ART_BIN:?ART_BIN is required}"
root="$(mktemp -d "${TMPDIR:-/tmp}/art-backup-e2e.XXXXXX")"
trap 'find "$root" -depth -delete 2>/dev/null || true' EXIT
source_home="$root/source-home"; bare="$root/remote.git"; repo="$root/repo"; clone="$root/clone"; restored="$root/restored"
"$art_bin" --home "$source_home" init --confirm >/dev/null
"$art_bin" --home "$source_home" agent create --id codex-backup-test --host codex >/dev/null
printf '%s\n' '# Backup recovery knowledge' 'A durable shared recovery fact.' > "$root/source.md"
source_sha="$(shasum -a 256 "$root/source.md" | awk '{print $1}')"
proposal_json="$("$art_bin" --home "$source_home" knowledge proposal compose-file --agent codex-backup-test --knowledge-key backup.recovery.test --title 'Backup Recovery Test' --applicability 'shell disaster recovery test' --markdown-file "$root/source.md" --source-id test-backup-recovery --source-sha256 "$source_sha" --idempotency-key backup-recovery-test)"
proposal_id="$(jq -r .id <<<"$proposal_json")"
"$art_bin" --home "$source_home" knowledge review approve "$proposal_id" --revision 1 --reason 'reviewed shell fixture' >/dev/null
first_edition_json="$("$art_bin" --home "$source_home" knowledge publish "$proposal_id" --revision 1 --confirm)"
first_edition_id="$(jq -r .edition_id <<<"$first_edition_json")"
printf '%s\n' '# Active recovery knowledge' 'The active durable shared recovery fact.' > "$root/active.md"
active_sha="$(shasum -a 256 "$root/active.md" | awk '{print $1}')"
active_proposal="$("$art_bin" --home "$source_home" knowledge proposal compose-file --agent codex-backup-test --knowledge-key backup.recovery.active --title 'Active Backup Recovery Test' --applicability 'shell disaster recovery test' --markdown-file "$root/active.md" --source-id test-backup-active --source-sha256 "$active_sha" --idempotency-key backup-recovery-active)"
active_proposal_id="$(jq -r .id <<<"$active_proposal")"
"$art_bin" --home "$source_home" knowledge review approve "$active_proposal_id" --revision 1 --reason 'reviewed active shell fixture' >/dev/null
"$art_bin" --home "$source_home" knowledge publish "$active_proposal_id" --revision 1 --confirm >/dev/null
"$art_bin" --home "$source_home" knowledge revoke "$first_edition_id" --reason 'exercise lifecycle recovery' --confirm >/dev/null
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
"$art_bin" --home "$restored" agent create --id dsh-recovery-test --host dsh >/dev/null
doctor_json="$("$art_bin" --home "$restored" doctor --agent dsh-recovery-test --json)"
[[ "$(jq -r '.status' <<<"$doctor_json")" == "ok" ]]
[[ "$(jq -r '.knowledge.projection_count' <<<"$doctor_json")" == "2" ]]
[[ "$(jq -r '.knowledge.current_edition_count' <<<"$doctor_json")" == "1" ]]
[[ "$(jq -r '.knowledge.event_files_verified' <<<"$doctor_json")" == "1" ]]
[[ "$(jq -r '.knowledge.search_index_aligned' <<<"$doctor_json")" == "true" ]]
recall_json="$("$art_bin" --home "$restored" recall 'durable shared recovery fact' --agent dsh-recovery-test --json)"
[[ "$(jq '.private_memories | length' <<<"$recall_json")" == "0" ]]
[[ "$(jq '.knowledge_editions | length' <<<"$recall_json")" -ge 1 ]]
[[ "$(sqlite3 "$restored/data/art/knowledge-vault/art-control.sqlite3" "SELECT value FROM backup_marker;")" == "CONTROL_PLAINTEXT_MUST_NOT_LEAK" ]]
[[ "$(sqlite3 "$restored/data/art/knowledge-vault/art-control.sqlite3" "SELECT COUNT(*) FROM publish_intents WHERE state='committed';")" == "2" ]]
[[ "$(sqlite3 "$restored/data/art/knowledge-vault/art-control.sqlite3" "SELECT COUNT(*) FROM event_intents WHERE state='committed';")" == "1" ]]
[[ "$(find "$repo" -type f -not -path "$repo/.git/*" -print | sed "s#^$repo/##" | sort | grep -c '^knowledge/editions/')" == "4" ]]
[[ "$(find "$repo" -type f -not -path "$repo/.git/*" -print | sed "s#^$repo/##" | sort | grep -c '^knowledge/events/')" == "1" ]]
echo "backup recovery shell E2E passed"
