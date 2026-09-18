use std::fs;

use art_agent_store::{
    AgentVault, AutoMemoryConfigStore, IntakeAttribution, IntakeDisposition, IntakeOrigin,
    MemoryIntakeRequest,
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

fn memory(agent: &AgentId, title: &str, statement: &str) -> MemoryArtifact {
    MemoryArtifact::new(
        agent.clone(),
        title,
        statement,
        MemoryPayload::Semantic(SemanticPayload {
            statement: statement.into(),
            applicability: "Use this bounded conclusion for the cited repository.".into(),
            exceptions: vec!["Revalidate after the cited evidence changes.".into()],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap()
}

fn receipt(agent: &AgentId, root: &std::path::Path, label: &str) -> SourceAnchor {
    let path = root.join(format!("{label}.receipt.json"));
    let bytes = serde_json::to_vec(&json!({
        "exit_code": 0,
        "evidence_scope": "unified intake contract"
    }))
    .unwrap();
    fs::write(&path, &bytes).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    SourceAnchor::new_with_source(
        agent.clone(),
        AnchorKind::TestReceipt,
        path.display().to_string(),
        Some("cargo-test".into()),
        Some(digest.clone()),
        None,
        json!({"exit_code":0,"evidence_scope":"unified intake contract","output_hash":digest}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap()
}

fn request(
    origin: Option<IntakeOrigin>,
    item: MemoryArtifact,
    anchors: Vec<SourceAnchor>,
    key: &str,
) -> MemoryIntakeRequest {
    MemoryIntakeRequest {
        capture_origin: origin,
        memory: item,
        anchors,
        idempotency_key: key.into(),
        attribution: IntakeAttribution::agent_asserted(None, None),
        request_basis: None,
        value_reason: Some("The verified conclusion prevents repeated investigation.".into()),
        target_memory_id: None,
        expected_revision: None,
        hook_trigger_receipt_id: None,
    }
}

#[test]
fn sha256_prefixed_evidence_verifies_without_rewriting_source_or_replay_hashes() {
    for kind in [
        AnchorKind::FileSnapshot,
        AnchorKind::CommandReceipt,
        AnchorKind::TestReceipt,
    ] {
        for origin in [
            IntakeOrigin::UserRequested,
            IntakeOrigin::AgentInitiated,
            IntakeOrigin::HookTriggered,
        ] {
            let root = tempdir().unwrap();
            let agent: AgentId = "codex-primary".parse().unwrap();
            let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
            let config = AutoMemoryConfigStore::new(root.path());
            config.set_enabled(true, "human:test").unwrap();
            let original = receipt(&agent, root.path(), "prefixed");
            let digest = format!("sha256:{}", original.source_digest.as_ref().unwrap());
            let anchor = SourceAnchor::new_with_source(
                agent.clone(),
                kind,
                original.locator,
                original.source_version,
                Some(digest.clone()),
                None,
                original.metadata,
                Sensitivity::Internal,
                Utc::now(),
            )
            .unwrap();
            let mut input = request(
                Some(origin),
                memory(
                    &agent,
                    "Prefix compatibility",
                    "The scoped verification accepts an algorithm-labeled SHA-256 digest.",
                ),
                vec![anchor],
                "prefixed",
            );
            if origin == IntakeOrigin::UserRequested {
                input.request_basis = Some("The user requested this verified conclusion.".into());
            }
            if origin == IntakeOrigin::HookTriggered {
                input.hook_trigger_receipt_id = Some(
                    vault
                        .claim_auto_memory_trigger(
                            &config,
                            "session",
                            "turn",
                            &"a".repeat(64),
                            Utc::now(),
                        )
                        .unwrap()
                        .receipt_id,
                );
            }
            let result = vault.intake(&config, input.clone()).unwrap();
            assert_eq!(
                result.disposition,
                IntakeDisposition::Activated,
                "{kind:?}/{origin:?}"
            );
            let stored = vault
                .export_record(result.memory_id.as_deref().unwrap())
                .unwrap();
            assert_eq!(
                stored.anchors[0].source_digest.as_deref(),
                Some(digest.as_str())
            );
            let replay = vault.intake(&config, input).unwrap();
            assert!(replay.replayed);
            assert_eq!(
                serde_json::to_value(replay.receipt).unwrap(),
                serde_json::to_value(result.receipt).unwrap()
            );
        }
    }
}

#[test]
fn labeled_sha256_evidence_still_rejects_wrong_algorithms_digests_and_changed_bytes() {
    for kind in [
        AnchorKind::FileSnapshot,
        AnchorKind::CommandReceipt,
        AnchorKind::TestReceipt,
    ] {
        for variant in [
            "wrong_algorithm",
            "double_prefix",
            "wrong_digest",
            "changed_bytes",
        ] {
            let root = tempdir().unwrap();
            let agent: AgentId = "codex-primary".parse().unwrap();
            let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
            let config = AutoMemoryConfigStore::new(root.path());
            config.set_enabled(true, "human:test").unwrap();
            let original = receipt(&agent, root.path(), variant);
            let hash = original.source_digest.as_deref().unwrap();
            let digest = match variant {
                "wrong_algorithm" => format!("sha512:{hash}"),
                "double_prefix" => format!("sha256:sha256:{hash}"),
                "wrong_digest" => format!("sha256:{}", "0".repeat(64)),
                _ => format!("sha256:{hash}"),
            };
            if variant == "changed_bytes" {
                fs::write(&original.locator, b"changed evidence").unwrap();
            }
            let anchor = SourceAnchor::new_with_source(
                agent.clone(),
                kind,
                original.locator,
                original.source_version,
                Some(digest),
                None,
                original.metadata,
                Sensitivity::Internal,
                Utc::now(),
            )
            .unwrap();
            let result = vault
                .intake(
                    &config,
                    request(
                        Some(IntakeOrigin::AgentInitiated),
                        memory(&agent, variant, variant),
                        vec![anchor],
                        variant,
                    ),
                )
                .unwrap();
            assert_eq!(
                result.disposition,
                IntakeDisposition::PendingReview,
                "{kind:?}/{variant}"
            );
        }
    }
}

#[test]
fn bilingual_corpus_preserves_unique_balanced_complete_guidance_cases() {
    use std::collections::{BTreeMap, BTreeSet};
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/corpora/unified-memory-value-v1.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut ids = BTreeSet::new();
    let mut languages = BTreeMap::new();
    let mut categories = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        assert!(
            !id.trim().is_empty() && ids.insert(id),
            "empty or duplicate case ID: {id}"
        );
        let language = case["language"].as_str().unwrap();
        assert!(
            matches!(language, "en" | "zh"),
            "{id}: unsupported language {language}"
        );
        *languages.entry(language).or_insert(0) += 1;
        categories.insert(case["category"].as_str().unwrap());
        let guidance = case["guidance"].as_str().unwrap();
        assert!(
            matches!(
                guidance,
                "submit" | "submit_for_review" | "skip" | "submit_revision"
            ),
            "{id}: unsupported guidance expectation {guidance}"
        );
    }
    assert!(cases.len() >= 60);
    assert_eq!(languages.len(), 2, "both English and Chinese are required");
    assert_eq!(
        languages["en"], languages["zh"],
        "English and Chinese coverage must be balanced"
    );
    assert_eq!(
        categories,
        BTreeSet::from([
            "verified",
            "uncertain",
            "negative",
            "malformed",
            "duplicate",
            "conflict",
            "sensitive",
            "revision"
        ])
    );
}

// These are program-policy trials. The fixture's guidance annotations are an
// independent rubric for real consuming-Agent trials, not a mock Agent oracle.
// In particular, weakly sourced progress can remain Candidate if an Agent
// wrongly submits it: semantic value selection belongs to the shared guidance.
#[test]
fn bilingual_corpus_has_identical_policy_outcomes_for_proactive_and_hook_intake() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/corpora/unified-memory-value-v1.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert!(cases.len() >= 60);
    for origin in [IntakeOrigin::AgentInitiated, IntakeOrigin::HookTriggered] {
        let mut outcomes = std::collections::BTreeMap::new();
        for case in cases {
            let root = tempdir().unwrap();
            let agent: AgentId = "codex-primary".parse().unwrap();
            let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
            let config = AutoMemoryConfigStore::new(root.path());
            config.set_enabled(true, "human:test").unwrap();
            let id = case["id"].as_str().unwrap();
            let category = case["category"].as_str().unwrap();
            let body = case["statement"].as_str().unwrap();
            let mut input = request(
                Some(origin),
                memory(&agent, id, body),
                vec![receipt(&agent, root.path(), id)],
                id,
            );
            if matches!(category, "uncertain" | "negative") {
                input.anchors = vec![
                    SourceAnchor::new(
                        agent.clone(),
                        AnchorKind::UserStatement,
                        format!("user:corpus:{id}"),
                        None,
                        json!({}),
                        Sensitivity::Internal,
                        Utc::now(),
                    )
                    .unwrap(),
                ];
            }
            let mut baseline = None;
            if matches!(category, "duplicate" | "conflict" | "revision") {
                let seeded = if category == "duplicate" {
                    input.memory.clone()
                } else {
                    memory(
                        &agent,
                        id,
                        "The previous reviewed baseline remains available.",
                    )
                };
                let mut seed = request(
                    Some(IntakeOrigin::UserRequested),
                    seeded.clone(),
                    vec![receipt(&agent, root.path(), "baseline")],
                    "baseline",
                );
                seed.request_basis = Some("User asked to preserve the reviewed baseline.".into());
                assert_eq!(
                    vault.intake(&config, seed).unwrap().disposition,
                    IntakeDisposition::Activated
                );
                baseline = Some(vault.read(&seeded.id).unwrap());
                if category == "revision" {
                    input.target_memory_id = Some(seeded.id);
                    input.expected_revision = Some(1);
                }
            }
            match case["variant"].as_str() {
                Some("missing_source") => input.anchors.clear(),
                Some("bad_anchor") => input.anchors[0].content_hash = "0".repeat(64),
                Some("empty_scope") => input.memory.scope = MemoryScope::Repository(String::new()),
                Some("bad_revision") => input.memory.revisions[0].revision = 2,
                Some("metadata") => {
                    input.anchors = vec![
                        SourceAnchor::new(
                            agent.clone(),
                            AnchorKind::UserStatement,
                            "user:sensitive-metadata",
                            None,
                            json!({"token":"synthetic-test-marker"}),
                            Sensitivity::Private,
                            Utc::now(),
                        )
                        .unwrap(),
                    ];
                }
                _ => {}
            }
            if origin == IntakeOrigin::HookTriggered {
                let claim = vault
                    .claim_auto_memory_trigger(
                        &config,
                        id,
                        "turn",
                        &hex::encode(Sha256::digest(id)),
                        Utc::now(),
                    )
                    .unwrap();
                input.hook_trigger_receipt_id = Some(claim.receipt_id);
            }
            let result = vault.intake(&config, input.clone());
            if category == "malformed" {
                assert!(
                    matches!(
                        result,
                        Err(ArtError::InvalidInput(_) | ArtError::SourceRequired)
                    ),
                    "{origin:?}/{id}: {result:?}"
                );
                assert_eq!(vault.count().unwrap(), 0, "{origin:?}/{id}");
                assert!(vault.intake_receipts().unwrap().is_empty());
                *outcomes.entry("error".to_owned()).or_insert(0) += 1;
                continue;
            }
            let result = result.unwrap();
            let actual = serde_json::to_value(result.disposition).unwrap();
            assert_eq!(actual, case["policy"], "{origin:?}/{id}");
            assert_eq!(result.origin, origin);
            assert_eq!(
                result.receipt.policy_version,
                art_agent_store::AUTO_MEMORY_POLICY_VERSION
            );
            let replay = vault.intake(&config, input).unwrap();
            assert!(replay.replayed, "{origin:?}/{id}");
            assert_eq!(replay.receipt.receipt_id, result.receipt.receipt_id);
            assert_eq!(replay.disposition, result.disposition);
            if let Some(baseline) = baseline {
                assert_eq!(
                    serde_json::to_value(vault.read(&baseline.id).unwrap()).unwrap(),
                    serde_json::to_value(&baseline).unwrap(),
                    "{origin:?}/{id}: preserved baseline"
                );
            }
            if category == "revision" {
                assert_eq!(vault.pending_revision_proposals().unwrap().len(), 1);
                assert!(result.proposal_id.is_some());
            } else if category == "sensitive" {
                assert_eq!(vault.count().unwrap(), 0);
                vault.checkpoint_wal().unwrap();
                let bytes = fs::read(vault.path()).unwrap();
                assert!(
                    !bytes
                        .windows(body.len())
                        .any(|window| window == body.as_bytes()),
                    "{origin:?}/{id}: sensitive body persisted"
                );
                let marker = b"synthetic-test-marker";
                assert!(
                    !bytes.windows(marker.len()).any(|window| window == marker),
                    "{origin:?}/{id}: sensitive marker persisted"
                );
            } else {
                let stored = vault.read(result.memory_id.as_deref().unwrap()).unwrap();
                let expected_status = if matches!(category, "verified" | "duplicate") {
                    MemoryStatus::Active
                } else {
                    MemoryStatus::Candidate
                };
                assert_eq!(stored.status, expected_status, "{origin:?}/{id}");
            }
            *outcomes
                .entry(actual.as_str().unwrap().to_owned())
                .or_insert(0) += 1;
        }
        eprintln!(
            "unified bilingual policy corpus {origin:?}: {} cases {outcomes:?}; guidance consuming-Agent trials are separate",
            cases.len()
        );
    }
}

#[test]
fn concurrent_proactive_retries_commit_one_receipt_and_preserve_it_across_restart() {
    use std::sync::{Arc, Barrier};
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:test").unwrap();
    let input = request(
        None,
        memory(
            &agent,
            "Concurrent retry",
            "One verified conclusion survives concurrent retries.",
        ),
        vec![receipt(&agent, root.path(), "concurrent")],
        "concurrent",
    );
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (path, agent, config, input, barrier) = (
                path.clone(),
                agent.clone(),
                config.clone(),
                input.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                let vault = AgentVault::open(path, agent).unwrap();
                barrier.wait();
                vault.intake(&config, input).unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
    assert!(
        results
            .iter()
            .all(|result| result.receipt.receipt_id == results[0].receipt.receipt_id)
    );
    let reopened = AgentVault::open(&path, agent).unwrap();
    assert_eq!(reopened.count().unwrap(), 1);
    assert_eq!(reopened.intake_receipts().unwrap().len(), 1);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| row
            .get::<_, u64>(
            0
        ))
        .unwrap(),
        1
    );
    config.set_enabled(false, "human:test").unwrap();
    let replay = reopened.intake(&config, input).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.disposition, IntakeDisposition::Activated);
    assert_eq!(replay.receipt.receipt_id, results[0].receipt.receipt_id);
}

#[test]
fn human_candidate_edit_rejects_sensitive_content_without_changing_candidate() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:test").unwrap();
    let item = memory(&agent, "Pending", "Use the repository procedure.");
    let id = item.id.clone();
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::UserStatement,
        "user:correction",
        None,
        json!({}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    vault
        .intake(&config, request(None, item, vec![anchor], "pending"))
        .unwrap();
    let payload = memory(&agent, "Sensitive", "password = test-only-sensitive-marker").payload;
    let result = vault.edit_and_confirm_auto_candidate(
        &id,
        1,
        "Review",
        "Review",
        payload,
        "human:test",
        "Reviewed",
    );
    assert!(matches!(result, Err(ArtError::InvalidInput(_))));
    let current = vault.read(&id).unwrap();
    assert_eq!(current.current_revision, 1);
    assert_eq!(current.status, MemoryStatus::Candidate);
}

#[test]
fn concurrent_mixed_origins_share_fallback_cap_and_restart_does_not_reset_it() {
    use std::sync::{Arc, Barrier};
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:test").unwrap();
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(config.path()).unwrap()).unwrap();
    settings["cooldown_seconds"] = json!(0);
    fs::write(config.path(), serde_json::to_vec(&settings).unwrap()).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|index| {
            let key = format!("mixed-{index}");
            let input = request(
                None,
                memory(
                    &agent,
                    &key,
                    &format!("Distinct verified conclusion {index}."),
                ),
                vec![receipt(&agent, root.path(), &key)],
                &key,
            );
            let (path, agent, config, barrier) =
                (path.clone(), agent.clone(), config.clone(), barrier.clone());
            std::thread::spawn(move || {
                let vault = AgentVault::open(path, agent).unwrap();
                barrier.wait();
                let mut input = input;
                if index % 2 == 0 {
                    let claim = vault
                        .claim_auto_memory_trigger_with_attribution(
                            &config,
                            &IntakeAttribution::agent_asserted(None, None),
                            &hex::encode(Sha256::digest(&key)),
                            Utc::now(),
                        )
                        .unwrap();
                    if !claim.accepted {
                        assert_eq!(claim.reason, "session_limit");
                        return IntakeDisposition::RateLimited;
                    }
                    input.capture_origin = Some(IntakeOrigin::HookTriggered);
                    input.hook_trigger_receipt_id = Some(claim.receipt_id);
                }
                vault.intake(&config, input).unwrap().disposition
            })
        })
        .collect();
    let outcomes: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == IntakeDisposition::Activated)
            .count(),
        3
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == IntakeDisposition::RateLimited)
            .count(),
        5
    );
    let reopened = AgentVault::open(&path, agent.clone()).unwrap();
    assert_eq!(reopened.count().unwrap(), 3);
    let mut after_restart = request(
        None,
        memory(
            &agent,
            "Restart",
            "Restart must preserve the exhausted admission budget.",
        ),
        vec![receipt(&agent, root.path(), "restart")],
        "restart",
    );
    assert_eq!(
        reopened
            .intake(&config, after_restart.clone())
            .unwrap()
            .disposition,
        IntakeDisposition::RateLimited
    );
    assert!(
        !reopened
            .claim_auto_memory_trigger_with_attribution(
                &config,
                &IntakeAttribution::agent_asserted(None, None),
                &hex::encode(Sha256::digest("restart")),
                Utc::now()
            )
            .unwrap()
            .accepted
    );
    config.set_enabled(false, "human:test").unwrap();
    after_restart.idempotency_key = "disabled".into();
    assert_eq!(
        reopened
            .intake(&config, after_restart.clone())
            .unwrap()
            .disposition,
        IntakeDisposition::Disabled
    );
    after_restart.idempotency_key = "explicit".into();
    after_restart.capture_origin = Some(IntakeOrigin::UserRequested);
    after_restart.request_basis =
        Some("User explicitly asked to retain the restart conclusion.".into());
    assert_eq!(
        reopened.intake(&config, after_restart).unwrap().disposition,
        IntakeDisposition::Activated
    );
    assert_eq!(reopened.count().unwrap(), 4);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| row
            .get::<_, u64>(
            0
        ))
        .unwrap(),
        3
    );
    assert!(reopened.integrity_check().unwrap());
}

#[test]
fn human_can_reject_obsolete_revision_proposal_without_changing_current_memory() {
    use art_agent_store::{MemoryReviewAction, MemoryRevisionReview};
    use art_domain::anchor::AssuranceOutcome;
    for advanced in [true, false] {
        let root = tempdir().unwrap();
        let agent: AgentId = "codex-primary".parse().unwrap();
        let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
        let config = AutoMemoryConfigStore::new(root.path());
        config.set_enabled(true, "human:test").unwrap();
        let original = memory(&agent, "Original", "The active procedure.");
        let id = original.id.clone();
        vault
            .capture(
                &original,
                &[receipt(&agent, root.path(), "original")],
                "original",
            )
            .unwrap();
        vault
            .assure(
                &id,
                1,
                AssuranceOutcome::Corroborated,
                "human:test",
                "Reviewed",
            )
            .unwrap();
        let mut input = request(
            None,
            memory(&agent, "Proposed", "The proposed procedure."),
            vec![receipt(&agent, root.path(), "proposal")],
            "proposal",
        );
        input.target_memory_id = Some(id.clone());
        input.expected_revision = Some(1);
        let result = vault.intake(&config, input).unwrap();
        if advanced {
            vault
                .revise(
                    &id,
                    1,
                    "New baseline",
                    "New baseline",
                    memory(&agent, "New baseline", "The newer procedure.").payload,
                    &[receipt(&agent, root.path(), "new")],
                    "New evidence",
                    "new",
                )
                .unwrap();
        } else {
            vault
                .assure(
                    &id,
                    1,
                    AssuranceOutcome::Disputed,
                    "human:test",
                    "Evidence changed",
                )
                .unwrap();
        }
        let before = serde_json::to_value(vault.read(&id).unwrap()).unwrap();
        let review = |action, expected_revision| MemoryRevisionReview {
            proposal_id: result.proposal_id.clone().unwrap(),
            expected_revision,
            action,
            reason: "Discard obsolete proposal".into(),
            title: None,
            summary: None,
            payload: None,
        };
        for action in [MemoryReviewAction::Confirm, MemoryReviewAction::EditConfirm] {
            assert!(matches!(
                vault.review_memory_revision(review(action, 1), "human:test"),
                Err(ArtError::SourceStale)
            ));
        }
        assert!(matches!(
            vault.review_memory_revision(review(MemoryReviewAction::Reject, 2), "human:test"),
            Err(ArtError::SourceStale)
        ));
        let rejected = vault
            .review_memory_revision(review(MemoryReviewAction::Reject, 1), "human:test")
            .unwrap();
        assert_eq!(rejected["status"], "rejected");
        assert!(vault.pending_revision_proposals().unwrap().is_empty());
        assert_eq!(
            serde_json::to_value(vault.read(&id).unwrap()).unwrap(),
            before
        );
        assert_eq!(vault.memory_revision_reviews().unwrap()[0], rejected);
    }
}

#[test]
fn intake_defaults_omitted_origin_and_rejects_an_unsubstantiated_user_request() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());

    let defaulted = vault
        .intake(
            &config,
            request(
                None,
                memory(&agent, "Default origin", "An omitted origin is automatic."),
                vec![receipt(&agent, root.path(), "defaulted")],
                "defaulted-origin",
            ),
        )
        .unwrap();
    assert_eq!(defaulted.origin, IntakeOrigin::AgentInitiated);
    assert_eq!(defaulted.disposition, IntakeDisposition::Disabled);

    let mut invalid = request(
        Some(IntakeOrigin::UserRequested),
        memory(
            &agent,
            "Missing basis",
            "A user request needs an auditable basis.",
        ),
        vec![receipt(&agent, root.path(), "missing-basis")],
        "missing-basis",
    );
    invalid.value_reason = None;
    let error = vault.intake(&config, invalid).unwrap_err();
    assert!(matches!(error, ArtError::InvalidInput(_)));
}

#[test]
fn user_requested_intake_activates_while_automatic_memory_is_disabled() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let mut input = request(
        Some(IntakeOrigin::UserRequested),
        memory(
            &agent,
            "Explicit repair",
            "The user asked to retain this verified repair.",
        ),
        vec![receipt(&agent, root.path(), "explicit-disabled")],
        "explicit-disabled",
    );
    input.request_basis = Some("User explicitly requested that this repair be remembered.".into());
    input.value_reason = None;

    let result = vault.intake(&config, input).unwrap();
    assert_eq!(result.disposition, IntakeDisposition::Activated);
    assert_eq!(
        vault
            .read(result.memory_id.as_deref().unwrap())
            .unwrap()
            .status,
        MemoryStatus::Active
    );
}

#[test]
fn automatic_origins_return_a_disabled_disposition_without_writing() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());

    for origin in [IntakeOrigin::AgentInitiated, IntakeOrigin::HookTriggered] {
        let result = vault
            .intake(
                &config,
                request(
                    Some(origin),
                    memory(
                        &agent,
                        "Automatic disabled",
                        "Disabled automatic intake stores nothing.",
                    ),
                    vec![receipt(
                        &agent,
                        root.path(),
                        &format!("disabled-{origin:?}"),
                    )],
                    &format!("disabled-{origin:?}"),
                ),
            )
            .unwrap();
        assert_eq!(result.disposition, IntakeDisposition::Disabled);
        assert!(result.memory_id.is_none());
    }
    assert_eq!(vault.count().unwrap(), 0);
}

#[test]
fn automatic_intake_uses_a_persistent_agent_day_fallback_budget() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();

    let first = vault
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                memory(
                    &agent,
                    "Fallback first",
                    "Fallback attribution is persistent.",
                ),
                vec![receipt(&agent, root.path(), "fallback-first")],
                "fallback-first",
            ),
        )
        .unwrap();
    assert_eq!(first.disposition, IntakeDisposition::Activated);
    assert!(first.receipt.attribution.degraded);
    assert!(first.receipt.budget_bucket.contains("agent-day"));

    drop(vault);
    let reopened = AgentVault::open(&path, agent.clone()).unwrap();
    let limited = reopened
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                memory(
                    &agent,
                    "Fallback second",
                    "Cooldown survives a process restart.",
                ),
                vec![receipt(&agent, root.path(), "fallback-second")],
                "fallback-second",
            ),
        )
        .unwrap();
    assert_eq!(limited.disposition, IntakeDisposition::RateLimited);
    assert_eq!(limited.reason, "cooldown");
}

#[test]
fn same_payload_across_origins_replays_or_matches_without_double_counting() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let item = memory(
        &agent,
        "Cross origin",
        "The same sourced fact has one memory.",
    );
    let anchors = vec![receipt(&agent, root.path(), "cross-origin")];

    let mut explicit = request(
        Some(IntakeOrigin::UserRequested),
        item.clone(),
        anchors.clone(),
        "same-key",
    );
    explicit.request_basis = Some("User asked to remember this sourced fact.".into());
    explicit.value_reason = None;
    let first = vault.intake(&config, explicit).unwrap();

    let replay = vault
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                item.clone(),
                anchors.clone(),
                "same-key",
            ),
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.memory_id, first.memory_id);
    assert_eq!(replay.origin, IntakeOrigin::UserRequested);

    let duplicate = vault
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                item,
                anchors,
                "different-key",
            ),
        )
        .unwrap();
    assert_eq!(duplicate.disposition, IntakeDisposition::Duplicate);
    assert_eq!(duplicate.memory_id, first.memory_id);
    assert_eq!(vault.count().unwrap(), 1);
}

#[test]
fn runtime_disable_stops_later_automatic_intake_and_preserves_existing_memory() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let admitted = vault
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                memory(
                    &agent,
                    "Before disable",
                    "The first automatic memory is retained.",
                ),
                vec![receipt(&agent, root.path(), "before-disable")],
                "before-disable",
            ),
        )
        .unwrap();
    config.set_enabled(false, "human:governance-ui").unwrap();
    let stopped = vault
        .intake(
            &config,
            request(
                Some(IntakeOrigin::AgentInitiated),
                memory(
                    &agent,
                    "After disable",
                    "This automatic memory must not be stored.",
                ),
                vec![receipt(&agent, root.path(), "after-disable")],
                "after-disable",
            ),
        )
        .unwrap();
    assert_eq!(stopped.disposition, IntakeDisposition::Disabled);
    assert_eq!(
        vault
            .read(admitted.memory_id.as_deref().unwrap())
            .unwrap()
            .status,
        MemoryStatus::Active
    );
    assert_eq!(vault.count().unwrap(), 1);
}

#[test]
fn automatic_revision_creates_a_pending_proposal_without_replacing_active_revision() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let mut initial = request(
        Some(IntakeOrigin::UserRequested),
        memory(
            &agent,
            "Stable guidance",
            "The active guidance remains the known good version.",
        ),
        vec![receipt(&agent, root.path(), "revision-initial")],
        "revision-initial",
    );
    initial.request_basis = Some("User asked to retain the known good guidance.".into());
    initial.value_reason = None;
    let active = vault.intake(&config, initial).unwrap();
    config.set_enabled(true, "human:governance-ui").unwrap();

    let mut proposed = request(
        Some(IntakeOrigin::AgentInitiated),
        memory(
            &agent,
            "Stable guidance",
            "A proposed automatic revision needs human confirmation.",
        ),
        vec![receipt(&agent, root.path(), "revision-proposed")],
        "revision-proposed",
    );
    proposed.target_memory_id = active.memory_id.clone();
    proposed.expected_revision = Some(1);
    let result = vault.intake(&config, proposed).unwrap();

    assert_eq!(result.disposition, IntakeDisposition::PendingReview);
    let stored = vault.read(active.memory_id.as_deref().unwrap()).unwrap();
    assert_eq!(stored.status, MemoryStatus::Active);
    assert_eq!(stored.current_revision, 1);
    assert_eq!(vault.pending_revision_proposals().unwrap().len(), 1);
}

#[test]
fn explicit_intake_rejects_sensitive_content_and_keeps_uncertain_evidence_pending() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    for (key, body, expected) in [
        (
            "sensitive",
            "password=not-a-real-secret",
            IntakeDisposition::Rejected,
        ),
        (
            "uncertain",
            "An unverified conclusion needs review.",
            IntakeDisposition::PendingReview,
        ),
    ] {
        let anchor = SourceAnchor::new(
            agent.clone(),
            AnchorKind::UserStatement,
            "user:bounded",
            None,
            json!({}),
            Sensitivity::Internal,
            Utc::now(),
        )
        .unwrap();
        let mut input = request(
            Some(IntakeOrigin::UserRequested),
            memory(&agent, key, body),
            vec![anchor],
            key,
        );
        input.request_basis = Some("User asked to retain this conclusion.".into());
        let result = vault.intake(&config, input).unwrap();
        assert_eq!(result.disposition, expected);
    }
    assert_eq!(vault.count().unwrap(), 1);
    assert!(!vault.list().unwrap()[0].summary.contains("password="));
}

#[test]
fn unified_keys_conflict_and_revision_targets_require_exact_versions() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let mut input = request(
        Some(IntakeOrigin::UserRequested),
        memory(&agent, "Original", "Original fact."),
        vec![receipt(&agent, root.path(), "original")],
        "key",
    );
    input.request_basis = Some("User asked to remember.".into());
    let initial = vault.intake(&config, input.clone()).unwrap();
    input.memory = memory(&agent, "Replacement", "Replacement fact.");
    assert!(matches!(
        vault.intake(&config, input.clone()),
        Err(ArtError::DuplicateConflict)
    ));
    input.idempotency_key = "revision".into();
    input.target_memory_id = initial.memory_id;
    assert!(matches!(
        vault.intake(&config, input.clone()),
        Err(ArtError::InvalidInput(_))
    ));
    input.expected_revision = Some(2);
    assert!(matches!(
        vault.intake(&config, input.clone()),
        Err(ArtError::SourceStale)
    ));
    input.expected_revision = Some(1);
    let revised = vault.intake(&config, input).unwrap();
    assert_eq!(revised.disposition, IntakeDisposition::Activated);
    assert_eq!(
        vault
            .read(revised.memory_id.as_deref().unwrap())
            .unwrap()
            .current_revision,
        2
    );
}

#[test]
fn migration_retains_legacy_receipts_and_makes_a_restorable_backup() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let item = memory(&agent, "Legacy", "Historical intent stays unknown.");
    vault.capture(&item, &[], "legacy").unwrap();
    vault.checkpoint_wal().unwrap();
    drop(vault);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("DROP TABLE IF EXISTS memory_intake_receipts; DROP TABLE IF EXISTS memory_intake_budget; DROP TABLE IF EXISTS memory_revision_proposals; UPDATE art_meta SET schema_version=3;").unwrap();
    drop(connection);
    let upgraded = AgentVault::open(&path, agent.clone()).unwrap();
    assert_eq!(upgraded.diagnostics().unwrap().schema_version, 4);
    let receipts = upgraded.intake_receipts().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].origin, IntakeOrigin::LegacyUnspecified);
    assert_eq!(upgraded.capture(&item, &[], "legacy").unwrap().id, item.id);
    let backup = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("pre-v4"))
        .unwrap();
    let restored_path = root.path().join("restored.sqlite3");
    fs::copy(backup, &restored_path).unwrap();
    let restored = AgentVault::open(&restored_path, agent).unwrap();
    assert_eq!(restored.read(&item.id).unwrap().summary, item.summary);
    assert!(restored.integrity_check().unwrap());
}

#[test]
fn hook_and_ordinary_intake_share_admissions_and_linked_turns() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let mut input = request(
        None,
        memory(
            &agent,
            "Ordinary",
            "Ordinary intake uses the shared budget.",
        ),
        vec![receipt(&agent, root.path(), "ordinary")],
        "ordinary",
    );
    input.attribution = IntakeAttribution::host_supplied("session-a".into(), Some("turn-a".into()));
    assert_eq!(
        vault.intake(&config, input).unwrap().disposition,
        IntakeDisposition::Activated
    );
    let hash = hex::encode(Sha256::digest(b"event"));
    let linked = vault
        .claim_auto_memory_trigger(&config, "session-a", "turn-a", &hash, Utc::now())
        .unwrap();
    assert!(!linked.accepted);
    assert_eq!(linked.reason, "turn_already_handled");
    let limited = vault
        .claim_auto_memory_trigger(&config, "session-a", "turn-b", &hash, Utc::now())
        .unwrap();
    assert!(!limited.accepted);
    assert_eq!(limited.reason, "cooldown");
    let claim = vault
        .claim_auto_memory_trigger(&config, "session-b", "turn-a", &hash, Utc::now())
        .unwrap();
    assert!(claim.accepted);
    let mut hook = request(
        Some(IntakeOrigin::HookTriggered),
        memory(&agent, "Hook", "The Hook claim funds its candidate."),
        vec![receipt(&agent, root.path(), "hook")],
        "hook",
    );
    hook.hook_trigger_receipt_id = Some(claim.receipt_id.clone());
    let result = vault.intake(&config, hook).unwrap();
    assert_eq!(result.disposition, IntakeDisposition::Activated);
    assert_eq!(
        result.receipt.budget_association.as_deref(),
        Some(claim.receipt_id.as_str())
    );
    let mut after = request(
        None,
        memory(
            &agent,
            "After hook",
            "The ordinary path sees Hook admissions.",
        ),
        vec![receipt(&agent, root.path(), "after-hook")],
        "after-hook",
    );
    after.attribution = IntakeAttribution::host_supplied("session-b".into(), Some("turn-b".into()));
    assert_eq!(
        vault.intake(&config, after).unwrap().disposition,
        IntakeDisposition::RateLimited
    );
    let connection = rusqlite::Connection::open(vault.path()).unwrap();
    let admissions: u32 = connection
        .query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(admissions, 2);
}

#[test]
fn fallback_budget_cannot_be_reset_by_asserting_new_session_ids() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    // Exercise the admission cap independently of cooldown, with persisted policy.
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(config.path()).unwrap()).unwrap();
    settings["cooldown_seconds"] = json!(0);
    fs::write(config.path(), serde_json::to_vec(&settings).unwrap()).unwrap();
    for i in 0..4 {
        let key = format!("fallback-{i}");
        let mut input = request(
            None,
            memory(&agent, &key, &format!("Distinct conclusion number {i}.")),
            vec![receipt(&agent, root.path(), &key)],
            &key,
        );
        input.attribution =
            IntakeAttribution::agent_asserted(Some(format!("untrusted-{i}")), Some("turn".into()));
        let result = vault.intake(&config, input).unwrap();
        assert_eq!(
            result.disposition,
            if i < 3 {
                IntakeDisposition::Activated
            } else {
                IntakeDisposition::RateLimited
            }
        );
    }
}

#[test]
fn malformed_evidence_and_non_hook_trigger_identity_fail_closed() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let mut input = request(
        None,
        memory(
            &agent,
            "Malformed",
            "Sources must remain structurally valid.",
        ),
        vec![receipt(&agent, root.path(), "malformed")],
        "malformed",
    );
    input.anchors[0].locator.clear();
    assert!(matches!(
        vault.intake(&config, input),
        Err(ArtError::InvalidInput(_))
    ));
    let mut input = request(
        None,
        memory(
            &agent,
            "Spoof",
            "Only Hook intake may attach trigger receipts.",
        ),
        vec![receipt(&agent, root.path(), "spoof")],
        "spoof",
    );
    input.hook_trigger_receipt_id = Some("unrelated-trigger".into());
    assert!(matches!(
        vault.intake(&config, input),
        Err(ArtError::InvalidInput(_))
    ));
    assert_eq!(vault.count().unwrap(), 0);
}

#[test]
fn historical_hook_receipts_migrate_without_losing_replay_or_origin() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let hash = hex::encode(Sha256::digest(b"old-hook"));
    let trigger = vault
        .claim_auto_memory_trigger(&config, "session", "turn", &hash, Utc::now())
        .unwrap();
    let item = memory(&agent, "Old Hook", "A historic receipt remains replayable.");
    let anchors = vec![receipt(&agent, root.path(), "old-hook")];
    let first = vault
        .submit_auto_candidate(&config, &item, &anchors, &trigger.receipt_id, 0)
        .unwrap();
    vault.checkpoint_wal().unwrap();
    drop(vault);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("DROP TABLE memory_intake_receipts; DROP TABLE memory_intake_budget; DROP TABLE memory_revision_proposals; UPDATE art_meta SET schema_version=3;").unwrap();
    drop(connection);
    let restored = AgentVault::open(&path, agent).unwrap();
    let replay = restored
        .submit_auto_candidate(&config, &item, &anchors, &trigger.receipt_id, 0)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.receipt_id, first.receipt_id);
    assert_eq!(
        restored.intake_receipts().unwrap()[0].origin,
        IntakeOrigin::LegacyUnspecified
    );
}

#[test]
fn a_hook_duplicate_does_not_spend_a_second_admission() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let item = memory(
        &agent,
        "One fact",
        "Duplicate intake does not spend another admission.",
    );
    let anchors = vec![receipt(&agent, root.path(), "one-fact")];
    let mut input = request(None, item.clone(), anchors.clone(), "ordinary");
    input.attribution =
        IntakeAttribution::host_supplied("ordinary-session".into(), Some("turn".into()));
    vault.intake(&config, input).unwrap();
    let hash = hex::encode(Sha256::digest(b"duplicate-hook"));
    let trigger = vault
        .claim_auto_memory_trigger(&config, "hook-session", "turn", &hash, Utc::now())
        .unwrap();
    assert!(trigger.accepted);
    let result = vault
        .submit_auto_candidate(&config, &item, &anchors, &trigger.receipt_id, 0)
        .unwrap();
    assert_eq!(
        result.outcome,
        art_agent_store::AutoMemoryOutcome::Duplicate
    );
    let connection = rusqlite::Connection::open(vault.path()).unwrap();
    let count: u32 = connection
        .query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    drop(connection);
    drop(vault);
    let reopened = AgentVault::open(root.path().join("agent.sqlite3"), agent).unwrap();
    let connection = rusqlite::Connection::open(reopened.path()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| row
                .get::<_, u32>(
                0
            ))
            .unwrap(),
        1
    );
}

#[test]
fn admission_receipt_and_candidate_roll_back_together_on_a_storage_conflict() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let anchor = receipt(&agent, root.path(), "shared-anchor");
    vault
        .capture(
            &memory(&agent, "Existing", "An existing record owns this anchor."),
            std::slice::from_ref(&anchor),
            "existing",
        )
        .unwrap();
    let input = request(
        None,
        memory(
            &agent,
            "Collision",
            "A second insert currently conflicts on anchor identity.",
        ),
        vec![anchor],
        "collision",
    );
    assert!(vault.intake(&config, input).is_err());
    assert_eq!(vault.count().unwrap(), 1);
    let connection = rusqlite::Connection::open(vault.path()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| row
                .get::<_, u32>(
                0
            ))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM memory_intake_receipts WHERE idempotency_key='collision'",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn revision_intake_cannot_rebind_an_existing_anchor_to_different_evidence() {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let anchor = receipt(&agent, root.path(), "original-evidence");
    let mut input = request(
        Some(IntakeOrigin::UserRequested),
        memory(
            &agent,
            "Original evidence",
            "Keep evidence identity intact.",
        ),
        vec![anchor.clone()],
        "original-evidence",
    );
    input.request_basis = Some("User requested this conclusion.".into());
    let first = vault.intake(&config, input).unwrap();
    let mut changed = receipt(&agent, root.path(), "changed-evidence");
    changed.id = anchor.id;
    let mut input = request(
        Some(IntakeOrigin::UserRequested),
        memory(
            &agent,
            "Changed evidence",
            "A new revision must link its actual evidence.",
        ),
        vec![changed],
        "changed-evidence",
    );
    input.request_basis = Some("User requested the revision.".into());
    input.target_memory_id = first.memory_id.clone();
    input.expected_revision = Some(1);
    assert!(matches!(
        vault.intake(&config, input),
        Err(ArtError::DuplicateConflict)
    ));
    assert_eq!(
        vault
            .read(first.memory_id.as_deref().unwrap())
            .unwrap()
            .current_revision,
        1
    );
}

fn rejects_corrupted_initial_revision(corruption: &str) {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let mut item = memory(&agent, corruption, "The submitted fact is harmless.");
    match corruption {
        "different_payload" => {
            item.revisions[0].payload =
                memory(&agent, "Different", "A different hidden conclusion.").payload;
        }
        "sensitive_payload" => {
            item.revisions[0].payload = memory(
                &agent,
                "Sensitive",
                "password=synthetic-hidden-revision-marker",
            )
            .payload;
        }
        "wrong_number" => item.revisions[0].revision = 2,
        _ => unreachable!(),
    }
    let mut input = request(
        Some(IntakeOrigin::UserRequested),
        item,
        vec![receipt(&agent, root.path(), corruption)],
        corruption,
    );
    input.request_basis = Some("User requested this bounded conclusion.".into());
    assert!(
        matches!(vault.intake(&config, input), Err(ArtError::InvalidInput(_))),
        "{corruption} must fail before storage"
    );
    assert_eq!(vault.count().unwrap(), 0);
    assert!(vault.intake_receipts().unwrap().is_empty());
    vault.checkpoint_wal().unwrap();
    assert!(
        !String::from_utf8_lossy(&fs::read(vault.path()).unwrap())
            .contains("synthetic-hidden-revision-marker")
    );
}

#[test]
fn initial_revision_payload_must_match_the_submitted_artifact() {
    rejects_corrupted_initial_revision("different_payload");
}

#[test]
fn initial_revision_sensitive_payload_is_never_persisted() {
    rejects_corrupted_initial_revision("sensitive_payload");
}

#[test]
fn initial_revision_number_must_match_the_submitted_artifact() {
    rejects_corrupted_initial_revision("wrong_number");
}

#[test]
fn migrated_capture_replays_through_intake_without_rewriting_historical_hashes() {
    for legacy_active in [false, true] {
        let root = tempdir().unwrap();
        let agent: AgentId = "codex-primary".parse().unwrap();
        let path = root.path().join("agent.sqlite3");
        let vault = AgentVault::open(&path, agent.clone()).unwrap();
        let config = AutoMemoryConfigStore::new(root.path());
        let item = memory(
            &agent,
            "Historical capture",
            "The original content remains replayable.",
        );
        let anchors = vec![receipt(&agent, root.path(), "historical-capture")];
        let mut historical = item.clone();
        if legacy_active {
            historical
                .transition(MemoryStatus::Active, Utc::now())
                .unwrap();
        }
        vault
            .capture(&historical, &anchors, "historical-key")
            .unwrap();
        vault.checkpoint_wal().unwrap();
        drop(vault);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let before: (String,String) = connection.query_row("SELECT id,payload_hash FROM capture_receipts WHERE idempotency_key='historical-key'",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        connection.execute_batch("DROP TABLE memory_intake_receipts; DROP TABLE memory_intake_budget; DROP TABLE memory_revision_proposals; UPDATE art_meta SET schema_version=3;").unwrap();
        drop(connection);
        let reopened = AgentVault::open(&path, agent).unwrap();
        let input = request(None, item, anchors, "historical-key");
        let replay = reopened.intake(&config, input.clone()).unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.receipt_id, before.0);
        assert_eq!(replay.receipt.payload_hash, before.1);
        assert_eq!(replay.origin, IntakeOrigin::LegacyUnspecified);
        assert_eq!(replay.memory_id.as_deref(), Some(historical.id.as_str()));
        let mut changed = input.clone();
        changed.memory.summary = "Changed content must still conflict.".into();
        assert!(matches!(
            reopened.intake(&config, changed),
            Err(ArtError::DuplicateConflict)
        ));
        let mut revision = input;
        revision.target_memory_id = Some(historical.id);
        revision.expected_revision = Some(1);
        assert!(matches!(
            reopened.intake(&config, revision),
            Err(ArtError::DuplicateConflict)
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        let after: (String,String) = connection.query_row("SELECT id,payload_hash FROM capture_receipts WHERE idempotency_key='historical-key'",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(before, after);
        assert_eq!(reopened.count().unwrap(), 1);
    }
}

fn distinct_submissions_in_one_turn_respect_budget(cooldown: u64) {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(config.path()).unwrap()).unwrap();
    settings["cooldown_seconds"] = json!(cooldown);
    fs::write(config.path(), serde_json::to_vec(&settings).unwrap()).unwrap();
    let allowed = if cooldown == 0 { 3 } else { 1 };
    for i in 0..4 {
        let key = format!("same-turn-{i}");
        let mut input = request(
            None,
            memory(&agent, &key, &format!("Distinct conclusion {i}.")),
            vec![receipt(&agent, root.path(), &key)],
            &key,
        );
        input.attribution =
            IntakeAttribution::host_supplied("trusted-session".into(), Some("same-turn".into()));
        let result = vault.intake(&config, input).unwrap();
        assert_eq!(
            result.disposition,
            if i < allowed {
                IntakeDisposition::Activated
            } else {
                IntakeDisposition::RateLimited
            }
        );
        if i >= allowed {
            assert_eq!(
                result.reason,
                if cooldown == 0 {
                    "session_limit"
                } else {
                    "cooldown"
                }
            );
        }
    }
    assert_eq!(vault.count().unwrap(), allowed);
}

#[test]
fn distinct_ordinary_submissions_in_one_trusted_turn_cannot_bypass_cooldown() {
    distinct_submissions_in_one_turn_respect_budget(600);
}

#[test]
fn distinct_ordinary_submissions_in_one_trusted_turn_cannot_bypass_session_cap() {
    distinct_submissions_in_one_turn_respect_budget(0);
}

#[test]
fn a_hook_reservation_can_fund_only_one_distinct_memory_across_origins() {
    for hook_first in [false, true] {
        let root = tempdir().unwrap();
        let agent: AgentId = "codex-primary".parse().unwrap();
        let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
        let config = AutoMemoryConfigStore::new(root.path());
        config.set_enabled(true, "human:governance-ui").unwrap();
        let hash = hex::encode(Sha256::digest(b"bounded-handoff"));
        let claim = vault
            .claim_auto_memory_trigger(&config, "session", "turn", &hash, Utc::now())
            .unwrap();
        assert!(claim.accepted);
        let mut first = request(
            None,
            memory(
                &agent,
                "First",
                "One reserved admission stores one conclusion.",
            ),
            vec![receipt(&agent, root.path(), "first")],
            "first",
        );
        first.attribution = IntakeAttribution::host_supplied("session".into(), Some("turn".into()));
        if hook_first {
            first.capture_origin = Some(IntakeOrigin::HookTriggered);
            first.hook_trigger_receipt_id = Some(claim.receipt_id.clone());
        }
        let admitted = vault.intake(&config, first).unwrap();
        assert_eq!(admitted.disposition, IntakeDisposition::Activated);
        assert_eq!(
            admitted.receipt.budget_association.as_deref(),
            Some(claim.receipt_id.as_str())
        );
        let mut second = request(
            None,
            memory(
                &agent,
                "Second",
                "A second distinct conclusion needs its own admission.",
            ),
            vec![receipt(&agent, root.path(), "second")],
            "second",
        );
        second.attribution =
            IntakeAttribution::host_supplied("session".into(), Some("turn".into()));
        if !hook_first {
            second.capture_origin = Some(IntakeOrigin::HookTriggered);
            second.hook_trigger_receipt_id = Some(claim.receipt_id.clone());
        }
        let limited = vault.intake(&config, second).unwrap();
        assert_eq!(limited.disposition, IntakeDisposition::RateLimited);
        assert_eq!(limited.reason, "cooldown");
        assert_eq!(vault.count().unwrap(), 1);
        let connection = rusqlite::Connection::open(vault.path()).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM memory_intake_budget", [], |row| row
                    .get::<_, u32>(
                    0
                ))
                .unwrap(),
            1
        );
    }
}

fn historical_capture_shape_replays_without_new_admission(legacy_active: bool) {
    let root = tempdir().unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let path = root.path().join("agent.sqlite3");
    let vault = AgentVault::open(&path, agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    let mut item = memory(
        &agent,
        "Historical shape",
        "Exact historical captures remain replayable.",
    );
    let anchors = if legacy_active {
        item.transition(MemoryStatus::Active, Utc::now()).unwrap();
        vec![receipt(&agent, root.path(), "historical-shape")]
    } else {
        vec![]
    };
    vault.capture(&item, &anchors, "historical-shape").unwrap();
    vault.checkpoint_wal().unwrap();
    drop(vault);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("DROP TABLE memory_intake_receipts; DROP TABLE memory_intake_budget; DROP TABLE memory_revision_proposals; UPDATE art_meta SET schema_version=3;").unwrap();
    drop(connection);
    let reopened = AgentVault::open(&path, agent).unwrap();
    let before = serde_json::to_string(&reopened.intake_receipts().unwrap()).unwrap();
    let original = request(None, item.clone(), anchors, "historical-shape");
    let replay = reopened.intake(&config, original.clone()).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.origin, IntakeOrigin::LegacyUnspecified);
    assert_eq!(replay.memory_id.as_deref(), Some(item.id.as_str()));
    let mut changed = original.clone();
    changed.memory.summary = "Different content is not a legacy replay.".into();
    assert!(matches!(
        reopened.intake(&config, changed),
        Err(ArtError::DuplicateConflict)
    ));
    let mut fresh = original;
    fresh.idempotency_key = "new-key".into();
    if legacy_active {
        assert!(matches!(
            reopened.intake(&config, fresh),
            Err(ArtError::InvalidInput(_))
        ));
    } else {
        assert!(matches!(
            reopened.intake(&config, fresh),
            Err(ArtError::SourceRequired)
        ));
    }
    assert_eq!(reopened.count().unwrap(), 1);
    assert_eq!(
        serde_json::to_string(&reopened.intake_receipts().unwrap()).unwrap(),
        before
    );
}

#[test]
fn historical_candidate_without_anchors_replays_before_new_intake_validation() {
    historical_capture_shape_replays_without_new_admission(false);
}

#[test]
fn historical_active_artifact_replays_before_new_intake_validation() {
    historical_capture_shape_replays_without_new_admission(true);
}
