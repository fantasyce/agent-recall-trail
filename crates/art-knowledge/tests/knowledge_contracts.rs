use std::{process::Command, str::FromStr};

use art_domain::{
    ArtError,
    agent::AgentId,
    knowledge::{
        KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ProposalStatus, ReviewActor,
        RiskLevel,
    },
    memory::Sensitivity,
};
use art_knowledge::KnowledgeVault;
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
    vault.publish(&proposal.id, proposal.revision, true).unwrap()
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
    let other = publish_search_fixture(
        &vault,
        &agent,
        "retrieval.gamma",
        "gamma",
        "gamma recovery",
    );

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
    drop(control);

    let recovered = KnowledgeVault::open(root.path(), [6_u8; 32]).unwrap();
    assert_eq!(
        recovered.current("recover.complete").unwrap().edition_id,
        edition.edition_id
    );
    assert_eq!(recovered.pending_recoveries().unwrap(), 0);
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
