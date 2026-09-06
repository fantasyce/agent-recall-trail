use std::{process::Command, str::FromStr, sync::Arc};

use art_domain::{
    ArtError,
    agent::AgentId,
    knowledge::{
        KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ProposalStatus, ReviewActor,
        RiskLevel,
    },
    memory::Sensitivity,
};
use art_knowledge::{DelegationMode, GovernanceSnapshot, KnowledgeVault};
use rusqlite::Connection;
use tempfile::tempdir;

fn source(agent: &AgentId) -> ProposalSourceLock {
    ProposalSourceLock {
        source_type: ProposalSourceType::PrivateMemory,
        owner_agent_id: Some(agent.clone()),
        source_id: "artm_private".into(),
        source_revision: Some(1),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: Some("b".repeat(64)),
        approved_excerpt_hash: Some("c".repeat(64)),
        use_grant_id: None,
    }
}

#[test]
fn delegation_policy_defaults_off_persists_and_stays_identity_scoped() {
    let root = tempdir().unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    let codex_host = "a".repeat(64);
    let other_host = "b".repeat(64);
    let vault = KnowledgeVault::open(root.path(), [43_u8; 32]).unwrap();

    assert_eq!(
        vault.delegation_mode(&codex, &codex_host).unwrap(),
        DelegationMode::HumanReview
    );
    vault
        .set_delegation_mode(
            &codex,
            &codex_host,
            DelegationMode::DelegatedLocal,
            "local_governance_ui",
        )
        .unwrap();
    assert_eq!(
        vault.delegation_mode(&dsh, &codex_host).unwrap(),
        DelegationMode::HumanReview
    );
    assert_eq!(
        vault.delegation_mode(&codex, &other_host).unwrap(),
        DelegationMode::HumanReview
    );
    drop(vault);

    let reopened = KnowledgeVault::open(root.path(), [43_u8; 32]).unwrap();
    assert_eq!(
        reopened.delegation_mode(&codex, &codex_host).unwrap(),
        DelegationMode::DelegatedLocal
    );
    reopened
        .set_delegation_mode(
            &codex,
            &codex_host,
            DelegationMode::HumanReview,
            "local_governance_ui",
        )
        .unwrap();
    assert_eq!(
        reopened.delegation_mode(&codex, &codex_host).unwrap(),
        DelegationMode::HumanReview
    );

    let connection = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    let event_count: u64 = connection
        .query_row("SELECT COUNT(*) FROM delegation_policy_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(event_count, 2);
    assert!(
        reopened
            .delegation_mode(&codex, "not-a-binding-hash")
            .is_err()
    );
}

fn published_fixture(
    key: &str,
    commitment_key: [u8; 32],
) -> (tempfile::TempDir, art_knowledge::EditionRecord) {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), commitment_key).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal(key, "Crash recovery", "durable body"),
            vec![source(&agent)],
            &format!("proposal-{key}"),
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    (root, edition)
}

fn publish_search_fixture(
    vault: &KnowledgeVault,
    agent: &AgentId,
    key: &str,
    title: &str,
    body: &str,
) -> art_knowledge::EditionRecord {
    let proposal = vault
        .propose(
            agent,
            KnowledgeDraft::minimal(key, title, body),
            vec![source(agent)],
            &format!("proposal-{key}"),
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "reviewed search fixture",
        )
        .unwrap();
    vault
        .publish(&proposal.id, proposal.revision, true)
        .unwrap()
}

#[test]
fn proposal_locks_exact_sources_and_agents_cannot_approve() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [7_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft {
                knowledge_key: "architecture.agent-isolation".into(),
                title: "ART Agent isolation".into(),
                applicability: "ART application interface".into(),
                markdown: "Each Agent has a separate private vault.".into(),
                sensitivity: Sensitivity::Internal,
                risk: art_domain::knowledge::RiskLevel::Normal,
            },
            vec![source(&agent)],
            "proposal-1",
        )
        .unwrap();
    assert_eq!(proposal.status, ProposalStatus::Submitted);
    assert!(matches!(
        vault.approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Agent(agent),
            "self approve"
        ),
        Err(ArtError::PermissionDenied(_))
    ));
}

#[test]
fn ranked_search_keeps_bm25_order_and_broad_terms() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [31_u8; 32]).unwrap();
    let phrase = publish_search_fixture(
        &vault,
        &agent,
        "retrieval.alpha-beta",
        "alpha beta",
        "alpha beta recovery",
    );
    let other =
        publish_search_fixture(&vault, &agent, "retrieval.gamma", "gamma", "gamma recovery");

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
    assert_eq!(ranked[0].edition.edition_id, phrase.edition_id);
    assert_eq!(ranked[0].lexical_rank, 1);
    assert_eq!(ranked[1].edition.edition_id, other.edition_id);
    assert_eq!(ranked[1].lexical_rank, 2);
}

#[test]
fn navigation_catalog_contains_only_current_editions_and_exact_applicability() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [42_u8; 32]).unwrap();
    let first = publish_search_fixture(
        &vault,
        &agent,
        "navigation.revoked",
        "First release guide",
        "first body",
    );
    let second = publish_search_fixture(
        &vault,
        &agent,
        "navigation.current",
        "Current release guide",
        "current body",
    );
    vault
        .revoke(&first.edition_id, "superseded fixture", true)
        .unwrap();

    assert_eq!(vault.rebuild_navigation().unwrap(), 1);
    let entries = vault.navigation_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].edition_id, second.edition_id);
    assert_ne!(entries[0].edition_id, first.edition_id);
    assert_eq!(entries[0].applicability, "local coding agents");
    assert!(entries[0].current);
    assert_eq!(entries[0].source_epoch, vault.index_epoch().unwrap());
    assert!(vault.navigation_aligned().unwrap());
    let diagnostics = vault.diagnostics().unwrap();
    assert!(diagnostics.navigation_aligned);
    assert_eq!(diagnostics.navigation_count, 1);
    publish_search_fixture(
        &vault,
        &agent,
        "navigation.late",
        "Late catalog entry",
        "invalidates the catalog epoch",
    );
    assert!(!vault.navigation_aligned().unwrap());
}

#[test]
fn proposal_idempotency_replays_the_same_payload_and_rejects_a_conflict() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [11_u8; 32]).unwrap();
    let first = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("idempotency.key", "One", "same"),
            vec![source(&agent)],
            "proposal-idempotency",
        )
        .unwrap();
    let replay = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("idempotency.key", "One", "same"),
            vec![source(&agent)],
            "proposal-idempotency",
        )
        .unwrap();
    assert_eq!(first.id, replay.id);
    assert!(matches!(
        vault.propose(
            &agent,
            KnowledgeDraft::minimal("idempotency.key", "Changed", "different"),
            vec![source(&agent)],
            "proposal-idempotency",
        ),
        Err(ArtError::DuplicateConflict)
    ));
}

#[test]
fn stale_source_invalidates_review_and_blocks_publish() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [8_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal(
                "operations.safe-shutdown",
                "Safe shutdown",
                "Send EOF, then verify exit.",
            ),
            vec![source(&agent)],
            "proposal-2",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "source checked",
        )
        .unwrap();
    vault
        .mark_source_changed(&proposal.id, &"d".repeat(64))
        .unwrap();
    assert!(matches!(
        vault.publish(&proposal.id, 1, true),
        Err(ArtError::SourceStale)
    ));
}

#[test]
fn a_materialized_proposal_is_terminal_and_cannot_be_reviewed_again() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [41_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("terminal.proposal", "Terminal", "one edition only"),
            vec![source(&agent)],
            "terminal-proposal",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "approved once",
        )
        .unwrap();
    vault
        .publish(&proposal.id, proposal.revision, true)
        .unwrap();
    assert!(matches!(
        vault.review(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "approved",
            "attempted replay",
        ),
        Err(ArtError::InvalidStateTransition)
    ));
    assert!(matches!(
        vault.publish(&proposal.id, proposal.revision, true),
        Err(ArtError::InvalidStateTransition)
    ));
}

#[test]
fn concurrent_exact_reviews_use_atomic_snapshot_compare_and_swap() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = Arc::new(KnowledgeVault::open(root.path(), [42_u8; 32]).unwrap());
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("atomic.review", "Atomic review", "review once"),
            vec![source(&agent)],
            "atomic-review",
        )
        .unwrap();
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    let handles: Vec<_> = ["reviewer-a", "reviewer-b"]
        .into_iter()
        .map(|actor| {
            let vault = Arc::clone(&vault);
            let snapshot = snapshot.clone();
            std::thread::spawn(move || {
                vault.review_exact(
                    &snapshot,
                    ReviewActor::Human(actor.into()),
                    "approved",
                    "concurrent exact review",
                )
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ArtError::SourceStale)))
            .count(),
        1
    );
}

#[test]
fn concurrent_same_key_publications_must_reconfirm_the_atomically_reserved_number() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = Arc::new(KnowledgeVault::open(root.path(), [43_u8; 32]).unwrap());
    let proposals: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|suffix| {
            let proposal = vault
                .propose(
                    &agent,
                    KnowledgeDraft::minimal("atomic.publish", format!("Edition {suffix}"), suffix),
                    vec![source(&agent)],
                    &format!("atomic-publish-{suffix}"),
                )
                .unwrap();
            vault
                .approve(
                    &proposal.id,
                    proposal.revision,
                    ReviewActor::Human("local-user".into()),
                    "approved",
                )
                .unwrap();
            GovernanceSnapshot::from_proposal(&vault.proposal(&proposal.id).unwrap())
        })
        .collect();
    assert_eq!(vault.next_edition_number("atomic.publish").unwrap(), 1);
    let handles: Vec<_> = proposals
        .iter()
        .cloned()
        .map(|snapshot| {
            let vault = Arc::clone(&vault);
            std::thread::spawn(move || vault.publish_exact(&snapshot, 1, true))
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ArtError::SourceStale)))
            .count(),
        1
    );
    assert_eq!(vault.next_edition_number("atomic.publish").unwrap(), 2);
    let remaining = proposals
        .iter()
        .find(|snapshot| {
            vault.proposal(&snapshot.proposal_id).unwrap().status == ProposalStatus::Approved
        })
        .unwrap();
    let second = vault.publish_exact(remaining, 2, true).unwrap();
    assert_eq!(second.edition_number, 2);
    assert_eq!(vault.current("atomic.publish").unwrap().edition_number, 2);
}

#[test]
fn partial_publish_recovery_releases_the_reserved_number_for_a_fresh_confirmation() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [44_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("recover.reserve", "Recover", "retry safely"),
            vec![source(&agent)],
            "recover-reserve",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "approved",
        )
        .unwrap();
    let edition_id = "arke_interrupted";
    let connection = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    connection.execute(
        "INSERT INTO publication_reservations(proposal_id,proposal_revision,knowledge_key,edition_number,edition_id,created_at) VALUES (?1,1,'recover.reserve',1,?2,'now')",
        rusqlite::params![proposal.id, edition_id],
    ).unwrap();
    connection.execute(
        "INSERT INTO publish_intents(id,proposal_id,proposal_revision,edition_id,target_dir,state,created_at,updated_at) VALUES ('arti_interrupted',?1,1,?2,?3,'prepared','now','now')",
        rusqlite::params![proposal.id, edition_id, root.path().join("editions/recover.reserve").to_string_lossy()],
    ).unwrap();
    drop(connection);
    drop(vault);

    let reopened = KnowledgeVault::open(root.path(), [44_u8; 32]).unwrap();
    assert_eq!(reopened.next_edition_number("recover.reserve").unwrap(), 1);
    let snapshot = GovernanceSnapshot::from_proposal(&reopened.proposal(&proposal.id).unwrap());
    let edition = reopened.publish_exact(&snapshot, 1, true).unwrap();
    assert_eq!(edition.edition_number, 1);
}

#[test]
fn targeted_failure_cleanup_never_touches_another_active_publication() {
    let root = tempdir().unwrap();
    let vault = KnowledgeVault::open(root.path(), [45_u8; 32]).unwrap();
    let target_a = root.path().join("editions/target-a");
    let target_b = root.path().join("editions/target-b");
    std::fs::create_dir_all(&target_a).unwrap();
    std::fs::create_dir_all(&target_b).unwrap();
    let file_a = target_a.join("1-arke_a.json");
    let file_b = target_b.join("1-arke_b.json");
    std::fs::write(&file_a, "partial a").unwrap();
    std::fs::write(&file_b, "active b").unwrap();
    let connection = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    for (proposal, key, edition, intent, target) in [
        ("artp_a", "target-a", "arke_a", "arti_a", &target_a),
        ("artp_b", "target-b", "arke_b", "arti_b", &target_b),
    ] {
        connection.execute(
            "INSERT INTO publication_reservations(proposal_id,proposal_revision,knowledge_key,edition_number,edition_id,created_at) VALUES (?1,1,?2,1,?3,'now')",
            rusqlite::params![proposal, key, edition],
        ).unwrap();
        connection.execute(
            "INSERT INTO publish_intents(id,proposal_id,proposal_revision,edition_id,target_dir,state,created_at,updated_at) VALUES (?1,?2,1,?3,?4,'prepared','now','now')",
            rusqlite::params![intent, proposal, edition, target.to_string_lossy()],
        ).unwrap();
    }
    drop(connection);

    vault
        .test_only_quarantine_publish_intent("arti_a", "arke_a", &target_a)
        .unwrap();
    assert!(!file_a.exists());
    assert!(file_b.exists());
    let connection = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    let state_b: String = connection
        .query_row(
            "SELECT state FROM publish_intents WHERE id='arti_b'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let reservation_b: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM publication_reservations WHERE edition_id='arke_b')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state_b, "prepared");
    assert!(reservation_b);
}

#[test]
fn editions_are_immutable_shareable_and_revocable_without_private_ids() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [9_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal(
                "retrieval.chinese",
                "Chinese retrieval",
                "Use exact, jieba, and CJK bigrams.",
            ),
            vec![source(&agent)],
            "proposal-3",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    let markdown = std::fs::read_to_string(&edition.markdown_path).unwrap();
    let manifest = std::fs::read_to_string(&edition.manifest_path).unwrap();
    assert!(markdown.contains("Use exact, jieba, and CJK bigrams."));
    assert!(!manifest.contains("codex-primary"));
    assert!(!manifest.contains("artm_private"));
    assert_eq!(
        vault.current("retrieval.chinese").unwrap().edition_id,
        edition.edition_id
    );
    vault.revoke(&edition.edition_id, "outdated", true).unwrap();
    assert!(matches!(
        vault.read(&edition.edition_id),
        Err(ArtError::NotFound)
    ));
    assert!(edition.markdown_path.exists());
}

#[test]
fn unconfirmed_publish_and_path_traversal_fail_closed() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [1_u8; 32]).unwrap();
    assert!(
        vault
            .propose(
                &agent,
                KnowledgeDraft::minimal("../escape", "bad", "bad"),
                vec![source(&agent)],
                "bad"
            )
            .is_err()
    );
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("safe.key", "Safe", "Reviewed content"),
            vec![source(&agent)],
            "safe",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    assert!(matches!(
        vault.publish(&proposal.id, 1, false),
        Err(ArtError::PermissionDenied(_))
    ));
}

#[test]
fn human_can_request_changes_or_reject_but_agent_cannot_review() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [2_u8; 32]).unwrap();
    let changes = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("review.changes", "Changes", "Needs evidence"),
            vec![source(&agent)],
            "review-changes",
        )
        .unwrap();
    vault
        .review(
            &changes.id,
            1,
            ReviewActor::Human("local-user".into()),
            "changes_requested",
            "add rollback evidence",
        )
        .unwrap();
    assert_eq!(
        vault.proposal(&changes.id).unwrap().status,
        ProposalStatus::ChangesRequested
    );

    let rejected = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("review.reject", "Reject", "Unsafe advice"),
            vec![source(&agent)],
            "review-reject",
        )
        .unwrap();
    assert!(matches!(
        vault.review(
            &rejected.id,
            1,
            ReviewActor::Agent(agent),
            "rejected",
            "self review"
        ),
        Err(ArtError::PermissionDenied(_))
    ));
    vault
        .review(
            &rejected.id,
            1,
            ReviewActor::Human("local-user".into()),
            "rejected",
            "unsafe advice",
        )
        .unwrap();
    assert_eq!(
        vault.proposal(&rejected.id).unwrap().status,
        ProposalStatus::Rejected
    );
}

#[test]
fn elevated_single_source_knowledge_requires_two_distinct_humans() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [24_u8; 32]).unwrap();
    let mut draft = KnowledgeDraft::minimal("risk.two-person", "Two person", "safe procedure");
    draft.risk = RiskLevel::High;
    let proposal = vault
        .propose(&agent, draft, vec![source(&agent)], "risk-two-person")
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("reviewer-a".into()),
            "first review",
        )
        .unwrap();
    assert_eq!(
        vault.proposal(&proposal.id).unwrap().status,
        ProposalStatus::UnderReview
    );
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("reviewer-a".into()),
            "same reviewer again",
        )
        .unwrap();
    assert_eq!(
        vault.proposal(&proposal.id).unwrap().status,
        ProposalStatus::UnderReview
    );
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("reviewer-b".into()),
            "independent review",
        )
        .unwrap();
    assert_eq!(
        vault.proposal(&proposal.id).unwrap().status,
        ProposalStatus::Approved
    );
}

#[test]
fn shared_projection_rebuilds_from_immutable_files_and_revocation_events() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [3_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("rebuild.current", "Rebuild", "Rebuild safely"),
            vec![source(&agent)],
            "rebuild-proposal",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    vault
        .revoke(&edition.edition_id, "invalidated", true)
        .unwrap();

    vault.test_only_clear_projection().unwrap();
    assert!(vault.list_current().unwrap().is_empty());
    assert_eq!(vault.rebuild_projection().unwrap(), 1);
    assert!(vault.list_current().unwrap().is_empty());
    assert!(matches!(
        vault.read(&edition.edition_id),
        Err(ArtError::NotFound)
    ));
}

#[test]
fn shared_search_projection_corruption_is_visible_and_rebuildable() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [23_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("search.rebuild", "检索重建", "共享索引恢复目标"),
            vec![source(&agent)],
            "search-rebuild",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    vault.publish(&proposal.id, 1, true).unwrap();
    let control = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    control.execute("DELETE FROM knowledge_fts", []).unwrap();
    drop(control);
    assert!(!vault.diagnostics().unwrap().search_index_aligned);
    assert!(
        vault
            .search_candidates(&["共享索引恢复目标".into()])
            .unwrap()
            .is_empty()
    );
    assert_eq!(vault.rebuild_search_index().unwrap(), 1);
    assert!(vault.diagnostics().unwrap().search_index_aligned);
    assert_eq!(
        vault
            .search_candidates(&["共享索引恢复目标".into()])
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_newer_same_key_edition_can_explicitly_supersede_an_older_edition() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [4_u8; 32]).unwrap();
    let mut editions = Vec::new();
    for (index, body) in ["first", "second"].into_iter().enumerate() {
        let proposal = vault
            .propose(
                &agent,
                KnowledgeDraft::minimal("supersede.key", "Supersede", body),
                vec![source(&agent)],
                &format!("supersede-{index}"),
            )
            .unwrap();
        vault
            .approve(
                &proposal.id,
                1,
                ReviewActor::Human("local-user".into()),
                "reviewed",
            )
            .unwrap();
        editions.push(vault.publish(&proposal.id, 1, true).unwrap());
    }
    vault
        .supersede(
            &editions[0].edition_id,
            &editions[1].edition_id,
            "new verification",
            true,
        )
        .unwrap();
    vault.rebuild_projection().unwrap();
    assert_eq!(
        vault.current("supersede.key").unwrap().edition_id,
        editions[1].edition_id
    );
}

#[cfg(unix)]
#[test]
fn publication_rejects_a_symbolic_link_in_the_edition_path() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [5_u8; 32]).unwrap();
    symlink(outside.path(), root.path().join("editions/symlink.key")).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("symlink.key", "Symlink", "blocked"),
            vec![source(&agent)],
            "symlink-proposal",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    assert!(matches!(
        vault.publish(&proposal.id, 1, true),
        Err(ArtError::PathConflict(_))
    ));
    assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[test]
fn opening_the_vault_completes_a_hash_valid_materialized_publish_intent() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [6_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("recover.complete", "Recover", "Committed files"),
            vec![source(&agent)],
            "recover-complete",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    vault.test_only_clear_projection().unwrap();
    let control = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    control
        .execute(
            "UPDATE publish_intents SET state='files_committed' WHERE edition_id=?1",
            [&edition.edition_id],
        )
        .unwrap();
    control
        .execute(
            "UPDATE knowledge_proposals SET status='approved' WHERE id=?1",
            [&proposal.id],
        )
        .unwrap();
    drop(control);

    let recovered = KnowledgeVault::open(root.path(), [6_u8; 32]).unwrap();
    assert_eq!(
        recovered.current("recover.complete").unwrap().edition_id,
        edition.edition_id
    );
    assert_eq!(recovered.pending_recoveries().unwrap(), 0);
    assert_eq!(
        recovered.proposal(&proposal.id).unwrap().status,
        ProposalStatus::Materialized
    );
}

#[test]
fn opening_the_vault_quarantines_a_partial_publish_without_exposing_it() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [10_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("recover.partial", "Recover", "Partial files"),
            vec![source(&agent)],
            "recover-partial",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    vault.test_only_clear_projection().unwrap();
    std::fs::remove_file(&edition.markdown_path).unwrap();
    let control = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    control
        .execute(
            "UPDATE publish_intents SET state='files_committed' WHERE edition_id=?1",
            [&edition.edition_id],
        )
        .unwrap();
    drop(control);

    let recovered = KnowledgeVault::open(root.path(), [10_u8; 32]).unwrap();
    assert!(matches!(
        recovered.current("recover.partial"),
        Err(ArtError::NotFound)
    ));
    assert_eq!(recovered.pending_recoveries().unwrap(), 1);
    let quarantine = root.path().join(".art/recovery");
    assert!(quarantine.exists());
    assert!(
        std::fs::read_dir(quarantine)
            .unwrap()
            .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .and_then(|value| value.to_str())
                == Some("json"))
    );
}

#[test]
fn corrupted_knowledge_files_are_degraded_and_never_returned_as_current() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [12_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("corruption.key", "Corruption", "original"),
            vec![source(&agent)],
            "corruption",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    std::fs::write(&edition.markdown_path, "malicious replacement").unwrap();
    assert!(matches!(
        vault.read(&edition.edition_id),
        Err(ArtError::IndexDegraded)
    ));
    assert!(!vault.diagnostics().unwrap().projection_hashes_ok);
    assert!(matches!(
        vault.rebuild_projection(),
        Err(ArtError::IndexDegraded)
    ));
}

#[test]
fn startup_reconciles_an_event_written_before_projection_commit_and_detects_event_loss() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [13_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("event.recovery", "Event recovery", "revoke me"),
            vec![source(&agent)],
            "event-recovery",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&proposal.id, 1, true).unwrap();
    vault.revoke(&edition.edition_id, "invalid", true).unwrap();
    let control = Connection::open(root.path().join("art-control.sqlite3")).unwrap();
    control.execute("DELETE FROM knowledge_events", []).unwrap();
    control
        .execute(
            "UPDATE edition_projections SET revoked=0,current=1 WHERE edition_id=?1",
            [&edition.edition_id],
        )
        .unwrap();
    drop(control);
    let recovered = KnowledgeVault::open(root.path(), [13_u8; 32]).unwrap();
    assert!(matches!(
        recovered.read(&edition.edition_id),
        Err(ArtError::NotFound)
    ));

    let event = std::fs::read_dir(root.path().join(".art/events"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::remove_file(event).unwrap();
    assert!(matches!(
        KnowledgeVault::open(root.path(), [13_u8; 32]),
        Err(ArtError::IndexDegraded)
    ));
}

#[test]
fn all_six_publish_crash_boundaries_recover_without_exposing_partial_knowledge() {
    for (index, case) in [
        "prepared",
        "temporary_written",
        "manifest_committed",
        "files_committed",
        "projection_committed",
        "intent_committed",
    ]
    .into_iter()
    .enumerate()
    {
        let key = format!("crash.case-{index}");
        let commitment_key = [u8::try_from(index + 30).unwrap(); 32];
        let (root, edition) = published_fixture(&key, commitment_key);
        let control_path = root.path().join("art-control.sqlite3");
        let control = Connection::open(&control_path).unwrap();
        match case {
            "prepared" => {
                std::fs::remove_file(&edition.markdown_path).unwrap();
                std::fs::remove_file(&edition.manifest_path).unwrap();
                control
                    .execute("DELETE FROM edition_projections", [])
                    .unwrap();
                control
                    .execute("UPDATE publish_intents SET state='prepared'", [])
                    .unwrap();
            }
            "temporary_written" => {
                std::fs::remove_file(&edition.markdown_path).unwrap();
                std::fs::rename(
                    &edition.manifest_path,
                    edition.manifest_path.with_extension("json.tmp"),
                )
                .unwrap();
                control
                    .execute("DELETE FROM edition_projections", [])
                    .unwrap();
                control
                    .execute("UPDATE publish_intents SET state='prepared'", [])
                    .unwrap();
            }
            "manifest_committed" => {
                std::fs::remove_file(&edition.markdown_path).unwrap();
                control
                    .execute("DELETE FROM edition_projections", [])
                    .unwrap();
                control
                    .execute("UPDATE publish_intents SET state='prepared'", [])
                    .unwrap();
            }
            "files_committed" => {
                control
                    .execute("DELETE FROM edition_projections", [])
                    .unwrap();
                control
                    .execute("UPDATE publish_intents SET state='files_committed'", [])
                    .unwrap();
            }
            "projection_committed" => {
                control
                    .execute(
                        "UPDATE publish_intents SET state='projection_committed'",
                        [],
                    )
                    .unwrap();
            }
            "intent_committed" => {}
            _ => unreachable!(),
        }
        drop(control);
        let recovered = KnowledgeVault::open(root.path(), commitment_key).unwrap();
        let should_be_visible = matches!(
            case,
            "files_committed" | "projection_committed" | "intent_committed"
        );
        if should_be_visible {
            assert_eq!(
                recovered.current(&key).unwrap().edition_id,
                edition.edition_id
            );
            assert_eq!(recovered.pending_recoveries().unwrap(), 0);
        } else {
            assert!(matches!(recovered.current(&key), Err(ArtError::NotFound)));
            assert_eq!(recovered.pending_recoveries().unwrap(), 1);
        }
    }
}

#[test]
fn publishing_never_stages_commits_switches_branches_or_configures_a_remote() {
    let root = tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    assert!(git(&["config", "user.name", "ART Test"]).status.success());
    assert!(
        git(&["config", "user.email", "art-test@example.invalid"])
            .status
            .success()
    );
    std::fs::write(root.path().join("baseline.txt"), "baseline\n").unwrap();
    assert!(git(&["add", "baseline.txt"]).status.success());
    assert!(git(&["commit", "-qm", "baseline"]).status.success());
    let head_before = git(&["rev-parse", "HEAD"]).stdout;
    let branch_before = git(&["branch", "--show-current"]).stdout;
    let remotes_before = git(&["remote"]).stdout;
    let index_before = std::fs::read(root.path().join(".git/index")).unwrap();

    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [42_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("git.untouched", "Git untouched", "working tree only"),
            vec![source(&agent)],
            "git-untouched",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    vault.publish(&proposal.id, 1, true).unwrap();

    assert_eq!(git(&["rev-parse", "HEAD"]).stdout, head_before);
    assert_eq!(git(&["branch", "--show-current"]).stdout, branch_before);
    assert_eq!(git(&["remote"]).stdout, remotes_before);
    assert_eq!(
        std::fs::read(root.path().join(".git/index")).unwrap(),
        index_before
    );
}
