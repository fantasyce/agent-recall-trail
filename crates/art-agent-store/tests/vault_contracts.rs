use std::{
    fs,
    str::FromStr,
    sync::{Arc, Barrier},
    thread,
};

use art_agent_store::AgentVault;
use art_domain::{
    ArtError,
    agent::AgentId,
    anchor::AssuranceOutcome,
    anchor::{AnchorKind, SourceAnchor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, SemanticPayload, Sensitivity,
    },
};
use chrono::Utc;
use serde_json::json;
use tempfile::tempdir;

fn memory(agent: &AgentId, statement: &str) -> MemoryArtifact {
    MemoryArtifact::new(
        agent.clone(),
        "ART isolation",
        statement,
        MemoryPayload::Semantic(SemanticPayload {
            statement: statement.into(),
            applicability: "ART application interface".into(),
            exceptions: vec![],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap()
}

fn anchor(agent: &AgentId) -> SourceAnchor {
    SourceAnchor::new(
        agent.clone(),
        AnchorKind::FileSnapshot,
        "repo:README.md",
        Some("ART has separate vaults".into()),
        json!({"digest": "sha256:abc"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap()
}

fn ranked_memory(agent: &AgentId, title: &str, statement: &str) -> MemoryArtifact {
    let mut artifact = MemoryArtifact::new(
        agent.clone(),
        title,
        statement,
        MemoryPayload::Semantic(SemanticPayload {
            statement: statement.into(),
            applicability: "ranked retrieval contract".into(),
            exceptions: vec![],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    artifact.transition(MemoryStatus::Active, Utc::now()).unwrap();
    artifact
}

#[test]
fn vault_binds_database_identity_and_rejects_a_different_agent() {
    let root = tempdir().unwrap();
    let path = root.path().join("art.sqlite3");
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    AgentVault::open(&path, codex).unwrap();
    assert!(matches!(
        AgentVault::open(&path, dsh),
        Err(ArtError::IdentityMismatch)
    ));
}

#[test]
fn capture_is_atomic_and_idempotent() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    let item = memory(&agent, "one file per Agent");
    let source = anchor(&agent);
    let first = vault
        .capture(&item, std::slice::from_ref(&source), "capture-1")
        .unwrap();
    let replay = vault.capture(&item, &[source], "capture-1").unwrap();
    assert_eq!(first.id, replay.id);
    assert_eq!(
        vault.read(&item.id).unwrap().current_hash,
        item.current_hash
    );
    assert_eq!(vault.count().unwrap(), 1);

    let changed = memory(&agent, "different payload");
    assert!(matches!(
        vault.capture(&changed, &[anchor(&agent)], "capture-1"),
        Err(ArtError::DuplicateConflict)
    ));
}

#[test]
fn ranked_search_keeps_bm25_order_and_broad_terms() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    let phrase = ranked_memory(&agent, "alpha beta", "alpha beta recovery");
    let other = ranked_memory(&agent, "gamma", "gamma recovery");
    vault
        .capture(&phrase, &[anchor(&agent)], "ranked-phrase")
        .unwrap();
    vault
        .capture(&other, &[anchor(&agent)], "ranked-other")
        .unwrap();

    let ranked = vault
        .search_ranked_candidates(
            &[
                "alpha beta".into(),
                "alpha".into(),
                "beta".into(),
                "gamma".into(),
            ],
            2,
        )
        .unwrap();

    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].artifact.id, phrase.id);
    assert_eq!(ranked[0].lexical_rank, 1);
    assert_eq!(ranked[1].artifact.id, other.id);
    assert_eq!(ranked[1].lexical_rank, 2);
}

#[test]
fn a_mid_transaction_anchor_conflict_leaves_no_orphan_memory_or_revision() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    let duplicated = anchor(&agent);
    let result = vault.capture(
        &memory(&agent, "must roll back"),
        &[duplicated.clone(), duplicated],
        "atomic-anchor-conflict",
    );
    assert!(result.is_err());
    assert_eq!(vault.count().unwrap(), 0);
    let diagnostics = vault.diagnostics().unwrap();
    assert_eq!(diagnostics.revision_count, 0);
    assert_eq!(diagnostics.anchor_count, 0);
    assert!(diagnostics.integrity_ok);
}

#[test]
fn concurrent_first_open_runs_one_identity_bound_migration() {
    let root = tempdir().unwrap();
    let path = Arc::new(root.path().join("art.sqlite3"));
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                AgentVault::open(path.as_ref(), AgentId::from_str("codex-primary").unwrap())
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    let vault =
        AgentVault::open(path.as_ref(), AgentId::from_str("codex-primary").unwrap()).unwrap();
    assert_eq!(vault.diagnostics().unwrap().bound_agent_id, "codex-primary");
}

#[test]
fn eight_connections_can_write_one_agent_without_crossing_vaults() {
    let root = tempdir().unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    let codex_path = root.path().join("codex.sqlite3");
    let dsh_path = root.path().join("dsh.sqlite3");
    let vault = Arc::new(AgentVault::open(&codex_path, codex.clone()).unwrap());
    AgentVault::open(&dsh_path, dsh.clone())
        .unwrap()
        .capture(&memory(&dsh, "DSH private"), &[anchor(&dsh)], "dsh-1")
        .unwrap();

    let handles: Vec<_> = (0..8)
        .map(|index| {
            let vault = Arc::clone(&vault);
            let agent = codex.clone();
            thread::spawn(move || {
                vault
                    .capture(
                        &memory(&agent, &format!("codex private {index}")),
                        &[anchor(&agent)],
                        &format!("codex-{index}"),
                    )
                    .unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(vault.count().unwrap(), 8);
    assert_eq!(
        AgentVault::open(&dsh_path, dsh).unwrap().count().unwrap(),
        1
    );
    assert_ne!(codex_path, dsh_path);
}

#[test]
fn unknown_newer_schema_fails_closed() {
    let root = tempdir().unwrap();
    let path = root.path().join("art.sqlite3");
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    vault.test_only_set_schema_version(999).unwrap();
    assert!(matches!(
        AgentVault::open(path, agent),
        Err(ArtError::SchemaTooNew)
    ));
}

#[test]
fn operator_lifecycle_is_append_only_and_preserves_superseded_records() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    let first = memory(&agent, "first claim");
    let second = memory(&agent, "replacement claim");
    vault
        .capture(&first, &[anchor(&agent)], "lifecycle-first")
        .unwrap();
    vault
        .capture(&second, &[anchor(&agent)], "lifecycle-second")
        .unwrap();

    vault
        .assure(
            &first.id,
            1,
            AssuranceOutcome::Corroborated,
            "human:local-user",
            "source checked",
        )
        .unwrap();
    assert_eq!(vault.read(&first.id).unwrap().status, MemoryStatus::Active);

    vault
        .supersede(&first.id, &second.id, "replacement verified")
        .unwrap();
    assert_eq!(
        vault.read(&first.id).unwrap().status,
        MemoryStatus::Superseded
    );
    assert!(vault.read(&second.id).is_ok());

    vault.archive(&first.id, "retention policy").unwrap();
    assert_eq!(
        vault.read(&first.id).unwrap().status,
        MemoryStatus::Archived
    );
}

#[test]
fn source_digest_change_appends_assurance_and_removes_memory_from_normal_use() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    let mut item = memory(&agent, "source can become stale");
    item.transition(MemoryStatus::Active, Utc::now()).unwrap();
    let source = anchor(&agent);
    vault
        .capture(&item, std::slice::from_ref(&source), "source-change")
        .unwrap();
    assert_eq!(
        vault
            .record_source_change(
                &source.id,
                Some("sha256:changed"),
                false,
                "human:local-user",
                "file changed",
            )
            .unwrap(),
        1
    );
    assert_eq!(vault.read(&item.id).unwrap().status, MemoryStatus::Disputed);
    assert!(matches!(
        vault.read_source_revision(&item.id, 1),
        Err(ArtError::SourceStale)
    ));
}

#[test]
fn private_export_round_trip_is_idempotent_and_hash_conflicts_fail_closed() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let source = AgentVault::open(root.path().join("source.sqlite3"), agent.clone()).unwrap();
    let item = memory(&agent, "portable exact snapshot");
    source
        .capture(&item, &[anchor(&agent)], "round-trip-source")
        .unwrap();
    let record = source.export_record(&item.id).unwrap();

    let destination =
        AgentVault::open(root.path().join("destination.sqlite3"), agent.clone()).unwrap();
    let imported = destination.import_record(&record).unwrap();
    let replay = destination.import_record(&record).unwrap();
    assert_eq!(imported.id, replay.id);
    assert_eq!(
        destination
            .export_record(&item.id)
            .unwrap()
            .artifact
            .current_hash,
        item.current_hash
    );

    let mut changed = record;
    changed.artifact.summary = "tampered summary".into();
    changed.artifact.payload = MemoryPayload::Semantic(SemanticPayload {
        statement: "tampered".into(),
        applicability: "ART application interface".into(),
        exceptions: vec![],
    });
    assert!(matches!(
        destination.import_record(&changed),
        Err(ArtError::InvalidInput(_))
    ));
}

#[test]
fn diagnostics_report_binding_integrity_wal_and_private_mode() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent).unwrap();
    let diagnostics = vault.diagnostics().unwrap();
    assert!(diagnostics.integrity_ok);
    assert_eq!(diagnostics.foreign_key_violations, 0);
    assert_eq!(diagnostics.database_kind, "agent-vault");
    assert_eq!(diagnostics.bound_agent_id, "codex-primary");
    assert_eq!(diagnostics.journal_mode, "wal");
    assert_eq!(diagnostics.migration_checksum.len(), 64);
    assert!(diagnostics.search_index_aligned);
    #[cfg(unix)]
    assert_eq!(diagnostics.file_mode, Some(0o600));
}

#[test]
fn private_search_projection_corruption_is_visible_and_rebuildable() {
    let root = tempdir().unwrap();
    let path = root.path().join("art.sqlite3");
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let item = memory(&agent, "可重建索引目标");
    vault
        .capture(&item, &[anchor(&agent)], "index-rebuild")
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("DELETE FROM memory_fts", []).unwrap();
    drop(connection);
    assert!(!vault.diagnostics().unwrap().search_index_aligned);
    assert!(
        vault
            .search_candidates(&["可重建索引目标".into()])
            .unwrap()
            .is_empty()
    );
    assert_eq!(vault.rebuild_search_index().unwrap(), 1);
    assert!(vault.diagnostics().unwrap().search_index_aligned);
    assert_eq!(
        vault
            .search_candidates(&["可重建索引目标".into()])
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn simulated_database_full_fails_without_committing_a_partial_memory() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("art.sqlite3"), agent.clone()).unwrap();
    vault.test_only_simulate_disk_full().unwrap();
    let before = vault.count().unwrap();
    let failure = vault.capture(
        &memory(&agent, "disk full"),
        &[anchor(&agent)],
        "database-full",
    );
    assert!(matches!(failure, Err(ArtError::Io(_))));
    assert_eq!(vault.count().unwrap(), before);
}

#[cfg(unix)]
#[test]
fn read_only_database_fails_closed() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    let path = root.path().join("art.sqlite3");
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    vault.checkpoint_wal().unwrap();
    drop(vault);
    let read_only = AgentVault::open(&path, agent.clone()).unwrap();
    fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    let result = read_only.capture(&memory(&agent, "read only"), &[anchor(&agent)], "read-only");
    fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(result, Err(ArtError::Io(_))));
}
