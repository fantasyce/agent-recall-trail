use std::fs;
use std::process::Command;
use std::sync::{Arc, Barrier};

use art_agent_store::{
    AUTO_MEMORY_POLICY_VERSION, AgentVault, AutoMemoryConfigStore, AutoMemoryOutcome,
};
use art_domain::{
    ArtError,
    agent::AgentId,
    anchor::{AnchorKind, SourceAnchor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, SemanticPayload, Sensitivity,
    },
};
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn candidate(agent: &AgentId, title: &str, statement: &str) -> MemoryArtifact {
    MemoryArtifact::new(
        agent.clone(),
        title,
        statement,
        MemoryPayload::Semantic(SemanticPayload {
            statement: statement.into(),
            applicability: "Use for this repository after the cited check.".into(),
            exceptions: vec!["Revalidate when the source changes.".into()],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap()
}

fn test_receipt(agent: &AgentId, root: &std::path::Path, label: &str) -> SourceAnchor {
    let receipt_path = root.join(format!("{label}.receipt.json"));
    let receipt = serde_json::to_vec(&json!({
        "exit_code": 0,
        "evidence_scope": "art-agent-store focused contract tests"
    }))
    .unwrap();
    fs::write(&receipt_path, &receipt).unwrap();
    let digest = hex::encode(Sha256::digest(&receipt));
    SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::TestReceipt,
        receipt_path.display().to_string(),
        Some("cargo-test-2026-09-15".into()),
        Some(digest.clone()),
        None,
        json!({"exit_code":0,"evidence_scope":"art-agent-store focused contract tests","output_hash":digest}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap()
}

fn admitted(vault: &AgentVault, config: &AutoMemoryConfigStore, label: &str) -> String {
    let event_hash = hex::encode(Sha256::digest(label.as_bytes()));
    vault
        .claim_auto_memory_trigger(config, label, "turn-1", &event_hash, Utc::now())
        .unwrap()
        .receipt_id
}

#[test]
fn disabled_switch_prevents_candidate_write_and_activation() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());

    let error = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Verified repair",
                "The repair passed its focused tests.",
            ),
            &[test_receipt(&agent, root.path(), "disabled")],
            "trigger-session-1-turn-1",
            0,
        )
        .unwrap_err();
    assert!(matches!(error, ArtError::PermissionDenied(_)));
    assert_eq!(vault.count().unwrap(), 0);
}

#[test]
fn verified_candidate_is_inserted_then_activated_by_system_policy() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = admitted(&vault, &config, "verified-trigger");

    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Verified repair",
                "The repair passed its focused tests.",
            ),
            &[test_receipt(&agent, root.path(), "verified")],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::Activated);
    assert_eq!(result.policy_version, AUTO_MEMORY_POLICY_VERSION);
    assert_eq!(result.reason, "verified_scoped_receipt");
    assert!(!result.replayed);
    let stored = vault.read(result.memory_id.as_deref().unwrap()).unwrap();
    assert_eq!(stored.status, MemoryStatus::Active);
}

#[test]
fn ambiguous_user_statement_and_conflicting_title_remain_pending() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::UserStatement,
        "user-statement:session-1:turn-3",
        None,
        json!({"signal":"correction"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let first_trigger = admitted(&vault, &config, "ambiguous-trigger");

    let first = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Corrected behavior",
                "Use the corrected local behavior.",
            ),
            &[anchor],
            &first_trigger,
            0,
        )
        .unwrap();
    assert_eq!(first.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(first.reason, "evidence_requires_human_review");
    assert_eq!(
        vault
            .read(first.memory_id.as_deref().unwrap())
            .unwrap()
            .status,
        MemoryStatus::Candidate
    );

    let second_trigger = admitted(&vault, &config, "conflict-trigger");
    let second = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Corrected behavior",
                "A different conclusion for the same topic.",
            ),
            &[test_receipt(&agent, root.path(), "conflict")],
            &second_trigger,
            0,
        )
        .unwrap();
    assert_eq!(second.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(second.reason, "possible_semantic_conflict");
}

#[test]
fn trigger_and_ordinal_are_idempotent_but_cannot_be_reused_for_other_content() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let memory = candidate(
        &agent,
        "Idempotent repair",
        "The same retry returns one memory.",
    );
    let anchors = vec![test_receipt(&agent, root.path(), "idempotent")];
    let trigger = admitted(&vault, &config, "idempotent-trigger");

    let first = vault
        .submit_auto_candidate(&config, &memory, &anchors, &trigger, 0)
        .unwrap();
    let replay = vault
        .submit_auto_candidate(&config, &memory, &anchors, &trigger, 0)
        .unwrap();
    assert_eq!(first.memory_id, replay.memory_id);
    assert!(replay.replayed);
    assert_eq!(vault.count().unwrap(), 1);

    let error = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Changed retry", "This is different content."),
            &anchors,
            &trigger,
            0,
        )
        .unwrap_err();
    assert!(matches!(error, ArtError::DuplicateConflict));
}

#[test]
fn exact_duplicate_returns_existing_active_memory_without_creating_another() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let first_memory = candidate(
        &agent,
        "Stable fact",
        "The exact verified fact is reusable.",
    );
    let first_trigger = admitted(&vault, &config, "duplicate-first-trigger");
    let first = vault
        .submit_auto_candidate(
            &config,
            &first_memory,
            &[test_receipt(&agent, root.path(), "duplicate-first")],
            &first_trigger,
            0,
        )
        .unwrap();
    let second_memory = candidate(
        &agent,
        "Stable fact",
        "The exact verified fact is reusable.",
    );
    let second_trigger = admitted(&vault, &config, "duplicate-second-trigger");
    let duplicate = vault
        .submit_auto_candidate(
            &config,
            &second_memory,
            &[test_receipt(&agent, root.path(), "duplicate-second")],
            &second_trigger,
            0,
        )
        .unwrap();
    assert_eq!(duplicate.outcome, AutoMemoryOutcome::Duplicate);
    assert_eq!(duplicate.memory_id, first.memory_id);
    assert_eq!(vault.count().unwrap(), 1);
}

#[test]
fn lexically_similar_same_scope_candidate_cannot_auto_activate() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();

    let first_trigger = admitted(&vault, &config, "similar-first-trigger");
    let first = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Rollback package digest policy",
                "Retain the previous verified package digest before replacing an installed runtime.",
            ),
            &[test_receipt(&agent, root.path(), "similar-first")],
            &first_trigger,
            0,
        )
        .unwrap();
    assert_eq!(first.outcome, AutoMemoryOutcome::Activated);

    let second_trigger = admitted(&vault, &config, "similar-second-trigger");
    let second = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Installed runtime rollback package policy",
                "Before replacing the installed runtime, retain its previous verified package digest.",
            ),
            &[test_receipt(&agent, root.path(), "similar-second")],
            &second_trigger,
            0,
        )
        .unwrap();

    assert_eq!(second.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(second.reason, "possible_semantic_duplicate_or_conflict");
    assert_eq!(
        vault
            .read(second.memory_id.as_deref().unwrap())
            .unwrap()
            .status,
        MemoryStatus::Candidate
    );
}

#[test]
fn unrelated_same_scope_candidate_still_auto_activates() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();

    let first_trigger = admitted(&vault, &config, "unrelated-first-trigger");
    vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Rollback package digest policy",
                "Retain the previous verified package digest before replacing an installed runtime.",
            ),
            &[test_receipt(&agent, root.path(), "unrelated-first")],
            &first_trigger,
            0,
        )
        .unwrap();

    let second_trigger = admitted(&vault, &config, "unrelated-second-trigger");
    let second = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Private socket permission",
                "Create the local control socket with mode 0600 before accepting requests.",
            ),
            &[test_receipt(&agent, root.path(), "unrelated-second")],
            &second_trigger,
            0,
        )
        .unwrap();

    assert_eq!(second.outcome, AutoMemoryOutcome::Activated);
}

#[test]
fn sensitive_candidate_is_rejected_without_persisting_its_body() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = admitted(&vault, &config, "sensitive-trigger");

    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Credential", "password = should-never-be-stored"),
            &[test_receipt(&agent, root.path(), "sensitive")],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::Rejected);
    assert_eq!(result.reason, "sensitive_content");
    assert!(result.memory_id.is_none());
    assert_eq!(vault.count().unwrap(), 0);
    let database = fs::read(vault.path()).unwrap();
    assert!(!String::from_utf8_lossy(&database).contains("should-never-be-stored"));
}

#[test]
fn sensitive_anchor_metadata_is_rejected_without_persisting_the_secret() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = admitted(&vault, &config, "sensitive-metadata-trigger");
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::UserStatement,
        "user-statement:safe",
        None,
        json!({"signal":"correction","api_key":"MUST_NOT_PERSIST_19ac"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Safe body", "The body itself is safe."),
            &[anchor],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::Rejected);
    assert_eq!(result.reason, "sensitive_content");
    let database = fs::read(vault.path()).unwrap();
    assert!(!String::from_utf8_lossy(&database).contains("MUST_NOT_PERSIST_19ac"));
}

#[test]
fn file_snapshot_activates_only_when_the_current_file_digest_matches() {
    let root = tempdir().unwrap();
    let source = root.path().join("verified.txt");
    fs::write(&source, b"verified-current-content").unwrap();
    let digest = hex::encode(Sha256::digest(b"verified-current-content"));
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let good = SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::FileSnapshot,
        source.display().to_string(),
        Some("1".into()),
        Some(digest),
        None,
        json!({"evidence_scope":"verified.txt"}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    let good_trigger = admitted(&vault, &config, "file-good-trigger");
    let activated = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Current file",
                "The current file matches its digest.",
            ),
            &[good],
            &good_trigger,
            0,
        )
        .unwrap();
    assert_eq!(activated.outcome, AutoMemoryOutcome::Activated);

    let stale = SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::FileSnapshot,
        source.display().to_string(),
        Some("1".into()),
        Some("0000000000000000000000000000000000000000000000000000000000000000".into()),
        None,
        json!({"evidence_scope":"verified.txt"}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    let stale_trigger = admitted(&vault, &config, "file-stale-trigger");
    let pending = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Stale file",
                "The stale digest cannot auto-activate.",
            ),
            &[stale],
            &stale_trigger,
            0,
        )
        .unwrap();
    assert_eq!(pending.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(pending.reason, "source_not_current");
}

#[test]
fn self_attested_execution_receipt_cannot_auto_activate() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let unverified = SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::TestReceipt,
        "receipt:self-attested-only",
        Some("claimed-pass".into()),
        Some("a".repeat(64)),
        None,
        json!({"exit_code":0,"evidence_scope":"claimed tests","output_hash":"a".repeat(64)}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    let trigger = admitted(&vault, &config, "self-attested-trigger");
    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Unverified receipt", "A claimed pass is not proof."),
            &[unverified],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(result.reason, "evidence_requires_human_review");
}

#[test]
fn trigger_receipts_enforce_cooldown_session_limit_replay_and_restart() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let start = Utc::now();
    let hash = |value: &str| hex::encode(Sha256::digest(value.as_bytes()));

    let first = vault
        .claim_auto_memory_trigger(&config, "session", "turn-1", &hash("one"), start)
        .unwrap();
    assert!(first.accepted);
    let replay = vault
        .claim_auto_memory_trigger(&config, "session", "turn-1", &hash("one"), start)
        .unwrap();
    assert!(replay.accepted && replay.replayed);
    let cooldown = vault
        .claim_auto_memory_trigger(
            &config,
            "session",
            "turn-2",
            &hash("two"),
            start + chrono::Duration::seconds(599),
        )
        .unwrap();
    assert!(!cooldown.accepted);
    assert_eq!(cooldown.reason, "cooldown");

    drop(vault);
    let reopened = AgentVault::open(&path, agent).unwrap();
    for (turn, seconds) in [("turn-3", 600), ("turn-4", 1200)] {
        let claim = reopened
            .claim_auto_memory_trigger(
                &config,
                "session",
                turn,
                &hash(turn),
                start + chrono::Duration::seconds(seconds),
            )
            .unwrap();
        assert!(claim.accepted, "{}", claim.reason);
    }
    let limited = reopened
        .claim_auto_memory_trigger(
            &config,
            "session",
            "turn-5",
            &hash("five"),
            start + chrono::Duration::seconds(1800),
        )
        .unwrap();
    assert!(!limited.accepted);
    assert_eq!(limited.reason, "session_limit");
}

#[test]
fn reenable_does_not_admit_a_candidate_formed_under_an_older_config_version() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = admitted(&vault, &config, "before-disable");
    config.set_enabled(false, "human:governance-ui").unwrap();
    config.set_enabled(true, "human:governance-ui").unwrap();

    let error = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Old candidate", "This must not be backfilled."),
            &[test_receipt(&agent, root.path(), "old-config")],
            &trigger,
            0,
        )
        .unwrap_err();
    assert!(matches!(error, ArtError::PermissionDenied(_)));
    assert_eq!(vault.count().unwrap(), 0);
}

#[test]
fn git_object_must_resolve_to_the_exact_current_source_version() {
    let root = tempdir().unwrap();
    let repository = root.path().join("repository");
    fs::create_dir(&repository).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    fs::write(repository.join("fact.txt"), b"verified git fact").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "fact.txt"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-c",
                "user.name=ART Test",
                "-c",
                "user.email=art@example.invalid",
                "commit",
                "-q",
                "-m",
                "fixture"
            ])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    let commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repository)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let anchor = SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::GitObject,
        "HEAD",
        Some(commit.trim().into()),
        None,
        None,
        json!({"repository_path": repository}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    let trigger = admitted(&vault, &config, "git-trigger");
    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(
                &agent,
                "Git-backed fact",
                "The exact Git object was verified.",
            ),
            &[anchor],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::Activated);
}

#[test]
fn otherwise_valid_evidence_older_than_thirty_days_stays_pending() {
    let root = tempdir().unwrap();
    let source = root.path().join("old-evidence.txt");
    fs::write(&source, b"old evidence").unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let anchor = SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::FileSnapshot,
        source.display().to_string(),
        Some("old".into()),
        Some(hex::encode(Sha256::digest(b"old evidence"))),
        None,
        json!({"evidence_scope":"old source"}),
        Sensitivity::Internal,
        Utc::now() - chrono::Duration::days(31),
    )
    .unwrap();
    let trigger = admitted(&vault, &config, "old-trigger");
    let result = vault
        .submit_auto_candidate(
            &config,
            &candidate(&agent, "Old evidence", "Old evidence cannot auto-activate."),
            &[anchor],
            &trigger,
            0,
        )
        .unwrap();
    assert_eq!(result.outcome, AutoMemoryOutcome::PendingReview);
    assert_eq!(result.reason, "source_not_current");
}

#[test]
fn concurrent_submission_retries_commit_exactly_one_candidate() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault =
        Arc::new(AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap());
    let config = Arc::new(AutoMemoryConfigStore::new(root.path()));
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = admitted(&vault, &config, "concurrent-trigger");
    let memory = Arc::new(candidate(
        &agent,
        "Concurrent candidate",
        "One retry result is stored.",
    ));
    let anchors = Arc::new(vec![test_receipt(&agent, root.path(), "concurrent")]);
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let vault = Arc::clone(&vault);
            let config = Arc::clone(&config);
            let memory = Arc::clone(&memory);
            let anchors = Arc::clone(&anchors);
            let barrier = Arc::clone(&barrier);
            let trigger = trigger.clone();
            std::thread::spawn(move || {
                barrier.wait();
                vault
                    .submit_auto_candidate(&config, &memory, &anchors, &trigger, 0)
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(
        results
            .iter()
            .all(|result| result.memory_id == results[0].memory_id)
    );
    assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
    assert_eq!(vault.count().unwrap(), 1);
}
