use std::str::FromStr;

use art_domain::agent::{AgentId, ArtPaths};
use art_domain::anchor::{
    AnchorKind, AssuranceDecision, AssuranceOutcome, SourceAnchor, anchor_set_hash,
};
use art_domain::memory::{
    DecisionPayload, EpisodePayload, MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus,
    ProcedurePayload, SemanticPayload, Sensitivity,
};
use chrono::Utc;
use proptest::prelude::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn agent_ids_are_canonical_and_reserved_names_are_rejected() {
    assert_eq!(
        AgentId::from_str("codex-primary").unwrap().as_str(),
        "codex-primary"
    );
    for invalid in [
        "DSH",
        "ab",
        "has space",
        "shared",
        "system",
        "all",
        "../codex",
    ] {
        assert!(AgentId::from_str(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn agent_vault_paths_are_physical_and_cannot_escape_root() {
    let root = tempdir().unwrap();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    assert_ne!(paths.agent_vault(&codex), paths.agent_vault(&dsh));
    assert!(paths.agent_vault(&codex).starts_with(root.path()));
    assert!(
        paths
            .ensure_managed_path(root.path().join("../escape"))
            .is_err()
    );
}

fn payloads() -> Vec<MemoryPayload> {
    vec![
        MemoryPayload::Episode(EpisodePayload {
            situation: "Codex MCP process leaked a file descriptor".into(),
            actions: vec!["closed the owned stdio child".into()],
            outcome: "descriptor count returned to baseline".into(),
            open_questions: vec!["verify child ownership before termination".into()],
        }),
        MemoryPayload::Semantic(SemanticPayload {
            statement: "ART keeps one physical vault per Agent".into(),
            applicability: "ART application interfaces".into(),
            exceptions: vec!["same-OS-user filesystem access".into()],
        }),
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["identify the exact child process".into()],
            steps: vec!["send EOF".into(), "wait up to three seconds".into()],
            verification: vec!["no child process remains".into()],
            rollback: vec!["reopen a new MCP session".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        MemoryPayload::Decision(DecisionPayload {
            rationale: "shared knowledge must not expose private memory".into(),
            decision: "publish a new immutable Knowledge Edition".into(),
            alternatives: vec!["shared memory database".into()],
            accepted_risks: vec!["human review remains mandatory".into()],
            revisit_when: Some("trust model changes".into()),
        }),
    ]
}

#[test]
fn all_four_payloads_validate_and_empty_required_fields_fail() {
    for payload in payloads() {
        assert!(payload.validate().is_ok());
    }
    let invalid = MemoryPayload::Semantic(SemanticPayload {
        statement: String::new(),
        applicability: "ART".into(),
        exceptions: vec![],
    });
    assert!(invalid.validate().is_err());
}

#[test]
fn memory_status_has_no_published_state_and_only_allows_declared_edges() {
    assert!(MemoryStatus::Candidate.can_transition_to(MemoryStatus::Active));
    assert!(MemoryStatus::Active.can_transition_to(MemoryStatus::Disputed));
    assert!(!MemoryStatus::Candidate.can_transition_to(MemoryStatus::Superseded));
    assert!(!MemoryStatus::Archived.can_transition_to(MemoryStatus::Active));
    assert!(
        serde_json::to_string(&MemoryStatus::Active)
            .unwrap()
            .to_lowercase()
            .find("publish")
            .is_none()
    );
}

#[test]
fn canonical_hash_ignores_json_key_order_and_changes_with_semantics() {
    let agent = AgentId::from_str("codex-primary").unwrap();
    let payload_a = MemoryPayload::Semantic(SemanticPayload {
        statement: "per-Agent vault".into(),
        applicability: "ART".into(),
        exceptions: vec![],
    });
    let mut memory = MemoryArtifact::new(
        agent,
        "Physical isolation",
        "Each Agent owns a separate file",
        payload_a,
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let original = memory.current_hash.clone();
    let one = art_domain::memory::canonical_json_hash(&json!({"b": 2, "a": 1}));
    let two = art_domain::memory::canonical_json_hash(&json!({"a": 1, "b": 2}));
    assert_eq!(one, two);
    memory
        .revise(
            MemoryPayload::Semantic(SemanticPayload {
                statement: "per-Agent physical SQLite vault".into(),
                applicability: "ART".into(),
                exceptions: vec![],
            }),
            "clarify storage",
            Utc::now(),
        )
        .unwrap();
    assert_eq!(memory.current_revision, 2);
    assert_ne!(original, memory.current_hash);
}

#[test]
fn anchors_reject_transcripts_secrets_and_unverified_boolean_receipts() {
    let agent = AgentId::from_str("codex-primary").unwrap();
    let secret = SourceAnchor::new(
        agent.clone(),
        AnchorKind::LogExcerpt,
        "local-log:1",
        Some(format!("{}: {} {}", "Authorization", "Bearer", "hidden")),
        json!({}),
        Sensitivity::Private,
        Utc::now(),
    );
    assert!(secret.is_err());

    let transcript = SourceAnchor::new(
        agent.clone(),
        AnchorKind::HostSessionRange,
        "session:1-20",
        Some("x".repeat(4097)),
        json!({"full_transcript": true}),
        Sensitivity::Private,
        Utc::now(),
    );
    assert!(transcript.is_err());

    let bare_pass = SourceAnchor::new(
        agent,
        AnchorKind::TestReceipt,
        "test:unit",
        Some("passed=true".into()),
        json!({"passed": true}),
        Sensitivity::Internal,
        Utc::now(),
    );
    assert!(bare_pass.is_err());
}

#[test]
fn all_eight_anchor_kinds_bind_safe_locator_version_digest_and_sensitivity() {
    let agent = AgentId::from_str("codex-primary").unwrap();
    for kind in [
        AnchorKind::HostSessionRange,
        AnchorKind::UserStatement,
        AnchorKind::FileSnapshot,
        AnchorKind::GitObject,
        AnchorKind::CommandReceipt,
        AnchorKind::TestReceipt,
        AnchorKind::LogExcerpt,
        AnchorKind::ExternalDocument,
    ] {
        let anchor = SourceAnchor::new_with_source(
            agent.clone(),
            kind,
            format!("source:{kind:?}"),
            Some("v1".into()),
            Some("sha256:abc".into()),
            Some("bounded excerpt".into()),
            json!({"exit_code":0,"output_hash":"abc"}),
            Sensitivity::Internal,
            Utc::now(),
        )
        .unwrap();
        assert_eq!(anchor.source_version.as_deref(), Some("v1"));
        assert_eq!(anchor.source_digest.as_deref(), Some("sha256:abc"));
        assert_eq!(anchor.sensitivity, Sensitivity::Internal);
        assert_eq!(anchor.content_hash.len(), 64);
    }
}

#[test]
fn assurance_is_bound_to_an_exact_revision_and_anchor_set() {
    let agent = AgentId::from_str("codex-primary").unwrap();
    let anchor = SourceAnchor::new(
        agent,
        AnchorKind::FileSnapshot,
        "repo:README.md",
        Some("ART is local-first".into()),
        json!({"digest": "sha256:abc", "version": 1}),
        Sensitivity::Internal,
        Utc::now(),
    )
    .unwrap();
    let set_hash = anchor_set_hash([&anchor]);
    let decision = AssuranceDecision::new(
        "artm_test",
        1,
        AssuranceOutcome::Corroborated,
        set_hash.clone(),
        "human:local",
        "file digest checked",
        Utc::now(),
    )
    .unwrap();
    assert_eq!(decision.memory_revision, 1);
    assert_eq!(decision.anchor_set_hash, set_hash);
}

proptest! {
    #[test]
    fn canonical_hash_is_stable_for_any_object_insertion_order(
        first in any::<i64>(),
        second in any::<i64>(),
    ) {
        let forward = art_domain::memory::canonical_json_hash(&json!({"a":first,"b":second}));
        let reverse = art_domain::memory::canonical_json_hash(&json!({"b":second,"a":first}));
        prop_assert_eq!(forward, reverse);
    }

    #[test]
    fn every_successful_revision_is_strictly_monotonic(
        claims in proptest::collection::vec("[a-zA-Z0-9][a-zA-Z0-9 -]{0,63}", 1..20),
    ) {
        let agent = AgentId::from_str("codex-primary").unwrap();
        let mut memory = MemoryArtifact::new(
            agent,
            "revision property",
            "revision must increase",
            MemoryPayload::Semantic(SemanticPayload {
                statement: "initial".into(),
                applicability: "property test".into(),
                exceptions: vec![],
            }),
            MemoryScope::User("local-user".into()),
            Sensitivity::Private,
            Utc::now(),
        ).unwrap();
        let mut previous = memory.current_revision;
        for claim in claims {
            memory.revise(
                MemoryPayload::Semantic(SemanticPayload {
                    statement: claim,
                    applicability: "property test".into(),
                    exceptions: vec![],
                }),
                "property revision",
                Utc::now(),
            ).unwrap();
            prop_assert_eq!(memory.current_revision, previous + 1);
            previous = memory.current_revision;
        }
    }
}
