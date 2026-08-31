use std::{fs, str::FromStr, sync::Arc};

use art_agent_store::AgentVault;
use art_domain::{
    agent::AgentId,
    anchor::{AnchorKind, SourceAnchor},
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, ProcedurePayload, Sensitivity,
    },
};
use art_knowledge::KnowledgeVault;
use art_retrieval::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, ProviderFingerprint, RankFusionPolicy,
    RecallDetail, RecallEngine, RecallOrigin, RecallRequest, RetrievalMode, SemanticProjection,
    SemanticRuntime, knowledge_semantic_documents, knowledge_semantic_path,
    private_semantic_documents, private_semantic_path,
};
use chrono::{Duration, Utc};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn recall_defaults_preserve_v020_lexical_behavior() {
    let request = RecallRequest::new("thunderbolt recovery");
    assert_eq!(request.mode, RetrievalMode::Lexical);
    assert_eq!(request.detail, RecallDetail::Recall);
}

#[test]
fn retrieval_modes_round_trip_as_stable_snake_case() {
    for (mode, expected) in [
        (RetrievalMode::Lexical, "\"lexical\""),
        (RetrievalMode::FullScan, "\"full_scan\""),
        (RetrievalMode::Semantic, "\"semantic\""),
        (RetrievalMode::Hybrid, "\"hybrid\""),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), expected);
    }
}

#[test]
fn versioned_rank_fusion_policy_loads_from_an_owner_only_file() {
    let root = tempdir().unwrap();
    let path = root.path().join("fusion.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "version":"art.rank-fusion.v1",
            "lexical_weight":0.9,
            "semantic_weight":1.1,
            "rrf_k":42
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let policy = RankFusionPolicy::load(&path).unwrap();
    assert_eq!(policy.version, "art.rank-fusion.v1");
    assert_eq!(policy.rrf_k, 42);

    fs::write(
        &path,
        br#"{"version":"art.rank-fusion.v1","lexical_weight":-1.0,"semantic_weight":1.0,"rrf_k":42}"#,
    )
    .unwrap();
    assert!(RankFusionPolicy::load(&path).is_err());
}

#[derive(Debug)]
struct SemanticRecallProvider {
    fingerprint: ProviderFingerprint,
    fail_query: bool,
}

impl EmbeddingProvider for SemanticRecallProvider {
    fn fingerprint(&self) -> ProviderFingerprint {
        self.fingerprint.clone()
    }

    fn embed(&self, input: EmbeddingInput<'_>) -> art_domain::ArtResult<Vec<Vec<f32>>> {
        match input {
            EmbeddingInput::Query(_) if self.fail_query => {
                Err(art_domain::ArtError::Internal("synthetic timeout".into()))
            }
            EmbeddingInput::Query(_) => Ok(vec![vec![0.0, 1.0, 0.0]]),
            EmbeddingInput::Documents(documents) => Ok(documents
                .iter()
                .map(|document| {
                    if document.contains("SEMANTIC_TARGET_DOCUMENT") {
                        vec![0.0, 1.0, 0.0]
                    } else {
                        vec![1.0, 0.0, 0.0]
                    }
                })
                .collect()),
        }
    }
}

fn semantic_endpoint(root: &std::path::Path) -> EmbeddingEndpoint {
    let path = root.join("semantic-endpoint.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema":"art.embedding.endpoint.v1",
            "protocol":"openai_compatible",
            "endpoint":"https://embedding.example.test",
            "model":"test/semantic-recall",
            "revision":"r1",
            "dimensions":3,
            "normalized":true,
            "timeout_ms":500
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    EmbeddingEndpoint::load(&path).unwrap()
}

#[test]
fn configured_semantic_mode_recalls_meaning_without_lexical_overlap() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent/art.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &private,
        &agent,
        "Cold restart procedure",
        "SEMANTIC_TARGET_DOCUMENT terminate orphan child cleanly",
        "semantic-target",
    );
    seed_memory(
        &private,
        &agent,
        "Exact lexical marker",
        "ordinary lexical document",
        "semantic-lexical-exact",
    );
    let candidate = seed_candidate_memory(
        &private,
        &agent,
        "Candidate semantic supplement",
        "ART_EXPLICIT_CANDIDATE_MARKER",
        "semantic-candidate",
    );
    let knowledge_root = root.path().join("knowledge");
    let knowledge = KnowledgeVault::open(&knowledge_root, [45; 32]).unwrap();
    let endpoint = semantic_endpoint(root.path());
    let provider = Arc::new(SemanticRecallProvider {
        fingerprint: endpoint.fingerprint(),
        fail_query: false,
    });
    let private_path = private_semantic_path(private.path());
    let knowledge_path = knowledge_semantic_path(&knowledge_root);
    SemanticProjection::rebuild(
        &private_path,
        &endpoint,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &private_semantic_documents(&private).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    SemanticProjection::rebuild(
        &knowledge_path,
        &endpoint,
        &knowledge.index_epoch().unwrap(),
        &knowledge_semantic_documents(&knowledge).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    let runtime = SemanticRuntime::open(
        &endpoint,
        provider,
        &private_path,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &knowledge_path,
        &knowledge.index_epoch().unwrap(),
    )
    .unwrap();
    let fallback_private = private.clone();
    let fallback_knowledge = knowledge.clone();
    let configurable_private = private.clone();
    let configurable_knowledge = knowledge.clone();
    let configurable_runtime = runtime.clone();
    let engine = RecallEngine::new(private, knowledge).with_semantic(runtime);

    let disabled = engine
        .clone()
        .with_semantic_unavailable("degraded", "rank_fusion_configuration_invalid");
    let disabled_bundle = disabled
        .recall(RecallRequest {
            mode: RetrievalMode::Semantic,
            ..RecallRequest::new("meaning only alias")
        })
        .unwrap();
    assert_eq!(disabled.vector_status(), "degraded");
    assert_eq!(disabled_bundle.effective_mode, RetrievalMode::Lexical);
    assert_eq!(
        disabled_bundle.fallback_reason.as_deref(),
        Some("rank_fusion_configuration_invalid")
    );

    let bundle = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Semantic,
            max_private_results: Some(1),
            ..RecallRequest::new("meaning only alias")
        })
        .unwrap();
    assert_eq!(bundle.effective_mode, RetrievalMode::Semantic);
    assert_eq!(bundle.vector_status, "ready");
    assert_eq!(bundle.private_memories.len(), 1);
    assert!(
        bundle.private_memories[0]
            .match_reasons
            .contains(&"semantic_rank".into())
    );
    let without_candidate = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Semantic,
            max_private_results: Some(2),
            ..RecallRequest::new("ART_EXPLICIT_CANDIDATE_MARKER")
        })
        .unwrap();
    assert!(
        without_candidate
            .private_memories
            .iter()
            .all(|item| item.subject_ref != format!("memory:{candidate}@1"))
    );
    let with_candidate = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Semantic,
            include_candidates: true,
            max_private_results: Some(3),
            ..RecallRequest::new("ART_EXPLICIT_CANDIDATE_MARKER")
        })
        .unwrap();
    let recalled_candidate = with_candidate
        .private_memories
        .iter()
        .find(|item| item.subject_ref == format!("memory:{candidate}@1"))
        .unwrap_or_else(|| {
            panic!(
                "explicitly included candidate should use the local lexical supplement: {with_candidate:#?}"
            )
        });
    assert!(
        recalled_candidate
            .match_reasons
            .contains(&"bm25_rank".into())
    );
    assert!(
        !recalled_candidate
            .match_reasons
            .contains(&"semantic_rank".into())
    );
    let candidate_position = with_candidate
        .private_memories
        .iter()
        .position(|item| item.subject_ref == format!("memory:{candidate}@1"))
        .unwrap();
    assert_eq!(candidate_position, 2);
    assert!(
        with_candidate.private_memories[..candidate_position]
            .iter()
            .all(|item| item.status == "active" && item.score > recalled_candidate.score),
        "a local Candidate supplement must not displace or outrank semantic Active memory: {with_candidate:#?}"
    );
    let hybrid = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Hybrid,
            ..RecallRequest::new("Exact lexical marker")
        })
        .unwrap();
    assert_eq!(hybrid.effective_mode, RetrievalMode::Hybrid);
    assert_eq!(hybrid.private_memories[0].title, "Exact lexical marker");
    let semantic_first = RecallEngine::new(configurable_private, configurable_knowledge)
        .with_semantic(configurable_runtime)
        .with_rank_fusion_policy(RankFusionPolicy {
            version: "art.rank-fusion.v1".into(),
            lexical_weight: 0.0,
            semantic_weight: 10.0,
            rrf_k: 1,
        })
        .unwrap()
        .recall(RecallRequest {
            mode: RetrievalMode::Hybrid,
            ..RecallRequest::new("Exact lexical marker")
        })
        .unwrap();
    assert_eq!(
        semantic_first.private_memories[0].title,
        "Cold restart procedure"
    );

    let lexical = RecallEngine::new(fallback_private.clone(), fallback_knowledge.clone())
        .recall(RecallRequest::new("Cold restart procedure"))
        .unwrap();
    let failing_provider = Arc::new(SemanticRecallProvider {
        fingerprint: endpoint.fingerprint(),
        fail_query: true,
    });
    let failing_runtime = SemanticRuntime::open(
        &endpoint,
        failing_provider,
        &private_path,
        &fallback_private.semantic_index_epoch(Utc::now()).unwrap(),
        &knowledge_path,
        &fallback_knowledge.index_epoch().unwrap(),
    )
    .unwrap();
    let fallback = RecallEngine::new(fallback_private, fallback_knowledge)
        .with_semantic(failing_runtime)
        .recall(RecallRequest {
            mode: RetrievalMode::Hybrid,
            ..RecallRequest::new("Cold restart procedure")
        })
        .unwrap();
    assert_eq!(fallback.effective_mode, RetrievalMode::Lexical);
    assert_eq!(fallback.vector_status, "degraded");
    assert_eq!(
        fallback.fallback_reason.as_deref(),
        Some("semantic_provider_failure")
    );
    assert_eq!(
        serde_json::to_value(&fallback.private_memories).unwrap(),
        serde_json::to_value(&lexical.private_memories).unwrap()
    );
}

#[test]
fn long_running_semantic_runtime_falls_back_when_canonical_epochs_change() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent/art.sqlite3"), agent.clone()).unwrap();
    let knowledge_root = root.path().join("knowledge");
    let knowledge = KnowledgeVault::open(&knowledge_root, [47; 32]).unwrap();
    let superseded = publish_knowledge_version(
        &knowledge,
        &agent,
        "runtime.epoch",
        "Superseded semantic edition",
        "SEMANTIC_TARGET_DOCUMENT obsolete procedure",
        "runtime-epoch-v1",
    );
    let endpoint = semantic_endpoint(root.path());
    let provider = Arc::new(SemanticRecallProvider {
        fingerprint: endpoint.fingerprint(),
        fail_query: false,
    });
    let private_path = private_semantic_path(private.path());
    let knowledge_path = knowledge_semantic_path(&knowledge_root);
    SemanticProjection::rebuild(
        &private_path,
        &endpoint,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &private_semantic_documents(&private).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    SemanticProjection::rebuild(
        &knowledge_path,
        &endpoint,
        &knowledge.index_epoch().unwrap(),
        &knowledge_semantic_documents(&knowledge).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    let runtime = SemanticRuntime::open(
        &endpoint,
        provider,
        &private_path,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &knowledge_path,
        &knowledge.index_epoch().unwrap(),
    )
    .unwrap();
    let lexical_private = private.clone();
    let lexical_knowledge = knowledge.clone();
    let engine = RecallEngine::new(private, knowledge.clone()).with_semantic(runtime);

    let replacement = publish_knowledge_version(
        &knowledge,
        &agent,
        "runtime.epoch",
        "Current replacement edition",
        "ART_CURRENT_REPLACEMENT_MARKER",
        "runtime-epoch-v2",
    );
    let lexical = RecallEngine::new(lexical_private, lexical_knowledge)
        .recall(RecallRequest::new("ART_CURRENT_REPLACEMENT_MARKER"))
        .unwrap();
    assert_eq!(lexical.knowledge_editions.len(), 1);
    assert_eq!(
        lexical.knowledge_editions[0].subject_ref,
        format!("knowledge:{replacement}")
    );

    let stale = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Hybrid,
            ..RecallRequest::new("ART_CURRENT_REPLACEMENT_MARKER")
        })
        .unwrap();
    assert_eq!(stale.effective_mode, RetrievalMode::Lexical);
    assert_eq!(stale.vector_status, "stale");
    assert_eq!(
        stale.fallback_reason.as_deref(),
        Some("semantic_projection_stale")
    );
    assert_eq!(engine.vector_status(), "stale");
    assert_eq!(
        serde_json::to_value(&stale.knowledge_editions).unwrap(),
        serde_json::to_value(&lexical.knowledge_editions).unwrap()
    );
    assert!(
        stale
            .knowledge_editions
            .iter()
            .all(|item| item.subject_ref != format!("knowledge:{superseded}"))
    );
}

#[test]
fn semantic_projection_documents_apply_private_governance_before_embedding() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &vault,
        &agent,
        "Eligible",
        "eligible semantic body",
        "semantic-eligible",
    );
    let disputed = seed_memory(
        &vault,
        &agent,
        "Disputed",
        "disputed body must not be embedded",
        "semantic-disputed",
    );
    vault.dispute(&disputed, "conflicting evidence").unwrap();
    seed_memory_with_validity(
        &vault,
        &agent,
        "Future",
        "future body must not be embedded early",
        "semantic-future-private",
        Some(Utc::now() + Duration::days(1)),
        None,
    );
    seed_memory_with_validity(
        &vault,
        &agent,
        "Expired",
        "expired body must not be sent to a provider",
        "semantic-expired-private",
        None,
        Some(Utc::now() - Duration::seconds(1)),
    );

    let documents = private_semantic_documents(&vault).unwrap();
    assert_eq!(documents.len(), 1);
    assert!(!documents[0].text.contains("disputed body"));
    assert!(!documents[0].text.contains("future body"));
    assert!(!documents[0].text.contains("expired body"));
}

#[test]
fn future_valid_private_memory_requires_reindex_and_falls_back_to_lexical() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent/art.sqlite3"), agent.clone()).unwrap();
    let memory_id = seed_memory_with_validity(
        &private,
        &agent,
        "Future semantic procedure",
        "SEMANTIC_TARGET_DOCUMENT FUTURE_SEMANTIC_MARKER",
        "future-semantic",
        Some(Utc::now() + Duration::seconds(1)),
        None,
    );
    let knowledge_root = root.path().join("knowledge");
    let knowledge = KnowledgeVault::open(&knowledge_root, [48; 32]).unwrap();
    let endpoint = semantic_endpoint(root.path());
    let provider = Arc::new(SemanticRecallProvider {
        fingerprint: endpoint.fingerprint(),
        fail_query: false,
    });
    let private_path = private_semantic_path(private.path());
    let knowledge_path = knowledge_semantic_path(&knowledge_root);
    SemanticProjection::rebuild(
        &private_path,
        &endpoint,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &private_semantic_documents(&private).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    SemanticProjection::rebuild(
        &knowledge_path,
        &endpoint,
        &knowledge.index_epoch().unwrap(),
        &knowledge_semantic_documents(&knowledge).unwrap(),
        provider.as_ref(),
    )
    .unwrap();
    let runtime = SemanticRuntime::open(
        &endpoint,
        provider,
        &private_path,
        &private.semantic_index_epoch(Utc::now()).unwrap(),
        &knowledge_path,
        &knowledge.index_epoch().unwrap(),
    )
    .unwrap();
    let engine = RecallEngine::new(private, knowledge).with_semantic(runtime);

    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let recalled = engine
        .recall(RecallRequest {
            mode: RetrievalMode::Semantic,
            max_private_results: Some(1),
            ..RecallRequest::new("FUTURE_SEMANTIC_MARKER")
        })
        .unwrap();
    assert_eq!(recalled.effective_mode, RetrievalMode::Lexical);
    assert_eq!(recalled.vector_status, "stale");
    assert_eq!(
        recalled.fallback_reason.as_deref(),
        Some("semantic_projection_stale")
    );
    assert_eq!(recalled.private_memories.len(), 1);
    assert_eq!(
        recalled.private_memories[0].subject_ref,
        format!("memory:{memory_id}@1")
    );
}

#[test]
fn unconfigured_semantic_modes_fall_back_to_byte_equivalent_lexical_items() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &vault,
        &agent,
        "Lexical fallback marker",
        "fallback remains exact and deterministic",
        "fallback-marker",
    );
    let disputed = seed_memory(
        &vault,
        &agent,
        "Lexical fallback marker",
        "conflicting evidence must remain visible as a caution",
        "fallback-disputed",
    );
    vault
        .dispute(&disputed, "synthetic conflicting evidence")
        .unwrap();
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [44; 32]).unwrap(),
    );
    let lexical = engine
        .recall(RecallRequest::new("Lexical fallback marker"))
        .unwrap();
    assert_eq!(
        lexical.cautions,
        [format!(
            "disputed private memory exists for subject {disputed}"
        )]
    );
    for mode in [RetrievalMode::Semantic, RetrievalMode::Hybrid] {
        let fallback = engine
            .recall(RecallRequest {
                mode,
                ..RecallRequest::new("Lexical fallback marker")
            })
            .unwrap();
        assert_eq!(fallback.requested_mode, mode);
        assert_eq!(fallback.effective_mode, RetrievalMode::Lexical);
        assert_eq!(fallback.vector_status, "unavailable");
        assert_eq!(
            fallback.fallback_reason.as_deref(),
            Some("semantic_unconfigured")
        );
        assert_eq!(
            serde_json::to_value(&fallback.private_memories).unwrap(),
            serde_json::to_value(&lexical.private_memories).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&fallback.knowledge_editions).unwrap(),
            serde_json::to_value(&lexical.knowledge_editions).unwrap()
        );
        assert_eq!(fallback.cautions, lexical.cautions);
    }
}

#[test]
fn lexical_admission_happens_before_the_bounded_rank_window() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let active = seed_memory(
        &vault,
        &agent,
        "Crowded lexical marker",
        "ART_ADMISSION_BEFORE_RANK_MARKER",
        "admission-active",
    );
    for index in 0..512 {
        seed_candidate_memory(
            &vault,
            &agent,
            "Crowded lexical marker",
            "ART_ADMISSION_BEFORE_RANK_MARKER",
            &format!("admission-candidate-{index}"),
        );
    }
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [49; 32]).unwrap(),
    );
    let recalled = engine
        .recall(RecallRequest {
            max_private_results: Some(1),
            ..RecallRequest::new("ART_ADMISSION_BEFORE_RANK_MARKER")
        })
        .unwrap();
    assert_eq!(recalled.private_memories.len(), 1);
    assert_eq!(
        recalled.private_memories[0].subject_ref,
        format!("memory:{active}@1")
    );
}

#[test]
fn full_scan_reads_both_canonical_stores_when_lexical_projections_are_empty() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &vault,
        &agent,
        "权威记录仍然存在",
        "ART_FULL_SCAN_CANONICAL_MARKER",
        "full-scan-canonical",
    );
    rusqlite::Connection::open(vault.path())
        .unwrap()
        .execute("DELETE FROM memory_fts", [])
        .unwrap();
    let knowledge_root = root.path().join("knowledge");
    let knowledge = KnowledgeVault::open(&knowledge_root, [41; 32]).unwrap();
    publish_knowledge(
        &knowledge,
        &agent,
        "full-scan.knowledge",
        "权威知识仍然存在",
        "ART_FULL_SCAN_CANONICAL_MARKER",
    );
    rusqlite::Connection::open(knowledge_root.join("art-control.sqlite3"))
        .unwrap()
        .execute("DELETE FROM knowledge_fts", [])
        .unwrap();
    let engine = RecallEngine::new(vault, knowledge);

    let lexical = engine
        .recall(RecallRequest::new("ART_FULL_SCAN_CANONICAL_MARKER"))
        .unwrap();
    assert!(lexical.private_memories.is_empty());
    assert!(lexical.knowledge_editions.is_empty());

    let full_scan = engine
        .recall(RecallRequest {
            mode: RetrievalMode::FullScan,
            ..RecallRequest::new("ART_FULL_SCAN_CANONICAL_MARKER")
        })
        .unwrap();
    assert_eq!(full_scan.private_memories.len(), 1);
    assert_eq!(full_scan.knowledge_editions.len(), 1);
    assert_eq!(full_scan.requested_mode, RetrievalMode::FullScan);
    assert_eq!(full_scan.effective_mode, RetrievalMode::FullScan);
    assert_eq!(full_scan.candidate_sources, ["canonical_full_scan"]);
    assert!(full_scan.fallback_reason.is_none());
}

#[test]
fn route_returns_bounded_navigation_metadata_without_memory_or_knowledge_bodies() {
    let root = tempdir().unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    let codex_vault = AgentVault::open(root.path().join("codex.sqlite3"), codex.clone()).unwrap();
    let dsh_vault = AgentVault::open(root.path().join("dsh.sqlite3"), dsh.clone()).unwrap();
    seed_memory(
        &codex_vault,
        &codex,
        "Release recovery route",
        "PRIVATE_BODY_MUST_NOT_APPEAR",
        "route-codex",
    );
    seed_memory(
        &dsh_vault,
        &dsh,
        "Release recovery DSH",
        "CROSS_AGENT_BODY_MUST_NOT_APPEAR",
        "route-dsh",
    );
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [43; 32]).unwrap();
    publish_knowledge(
        &knowledge,
        &codex,
        "release.shared-route",
        "Shared release route",
        "KNOWLEDGE_BODY_MUST_NOT_APPEAR",
    );
    let engine = RecallEngine::new(codex_vault.clone(), knowledge);

    seed_memory_with_validity(
        &codex_vault,
        &codex,
        "Future route marker",
        "FUTURE_ROUTE_BODY_MUST_NOT_APPEAR",
        "route-future",
        Some(Utc::now() + Duration::milliseconds(150)),
        None,
    );
    codex_vault.rebuild_navigation().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let future = engine
        .recall(RecallRequest {
            detail: RecallDetail::Route,
            ..RecallRequest::new("Future route marker")
        })
        .unwrap();
    assert!(
        future
            .navigation_topics
            .iter()
            .any(|topic| topic.title == "Future route marker")
    );
    assert!(
        !serde_json::to_string(&future)
            .unwrap()
            .contains("FUTURE_ROUTE_BODY")
    );

    let bundle = engine
        .recall(RecallRequest {
            detail: RecallDetail::Route,
            ..RecallRequest::new("release route")
        })
        .unwrap();

    assert!(bundle.private_memories.is_empty());
    assert!(bundle.knowledge_editions.is_empty());
    assert!(!bundle.navigation_topics.is_empty());
    assert!(bundle.navigation_topics.len() <= 12);
    assert!(
        bundle
            .navigation_topics
            .iter()
            .all(|topic| topic.subject_refs.len() <= 8)
    );
    assert_eq!(bundle.map_status, "ready");
    let encoded = serde_json::to_string(&bundle).unwrap();
    for forbidden in [
        "PRIVATE_BODY_MUST_NOT_APPEAR",
        "CROSS_AGENT_BODY_MUST_NOT_APPEAR",
        "KNOWLEDGE_BODY_MUST_NOT_APPEAR",
        "dsh-primary",
    ] {
        assert!(!encoded.contains(forbidden), "leaked {forbidden}");
    }

    rusqlite::Connection::open(codex_vault.path())
        .unwrap()
        .execute("DROP TABLE memory_navigation", [])
        .unwrap();
    let degraded = engine
        .recall(RecallRequest {
            detail: RecallDetail::Route,
            ..RecallRequest::new("release recovery")
        })
        .unwrap();
    assert_eq!(degraded.map_status, "degraded");
    assert!(
        degraded
            .candidate_sources
            .contains(&"private_canonical".into())
    );
    assert_eq!(
        degraded.fallback_reason.as_deref(),
        Some("navigation_projection_fallback")
    );
    assert!(
        degraded
            .navigation_topics
            .iter()
            .any(|topic| topic.lane == "private_memory")
    );

    rusqlite::Connection::open(root.path().join("knowledge/art-control.sqlite3"))
        .unwrap()
        .execute("DROP TABLE knowledge_navigation", [])
        .unwrap();
    let applicability_fallback = engine
        .recall(RecallRequest {
            detail: RecallDetail::Route,
            ..RecallRequest::new("local coding agents")
        })
        .unwrap();
    assert!(
        applicability_fallback
            .navigation_topics
            .iter()
            .any(|topic| topic.lane == "shared_knowledge")
    );
    assert!(
        applicability_fallback
            .candidate_sources
            .contains(&"shared_canonical".into())
    );
}

fn seed_memory(vault: &AgentVault, agent: &AgentId, title: &str, text: &str, key: &str) -> String {
    seed_memory_with_validity(vault, agent, title, text, key, None, None)
}

fn seed_memory_with_validity(
    vault: &AgentVault,
    agent: &AgentId,
    title: &str,
    text: &str,
    key: &str,
    valid_from: Option<chrono::DateTime<Utc>>,
    valid_until: Option<chrono::DateTime<Utc>>,
) -> String {
    let mut memory = MemoryArtifact::new(
        agent.clone(),
        title,
        text,
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["确认进程父子关系".into()],
            steps: vec![text.into()],
            verification: vec!["FD 数量回到基线".into()],
            rollback: vec!["重新建立 stdio 会话".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    memory.valid_from = valid_from;
    memory.valid_until = valid_until;
    memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::TestReceipt,
        "test:shutdown",
        Some("exit 0; output digest recorded".into()),
        json!({"exit_code":0,"output_hash":"abc"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault.capture(&memory, &[anchor], key).unwrap();
    memory.id
}

fn seed_candidate_memory(
    vault: &AgentVault,
    agent: &AgentId,
    title: &str,
    text: &str,
    key: &str,
) -> String {
    let memory = MemoryArtifact::new(
        agent.clone(),
        title,
        text,
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["candidate only".into()],
            steps: vec![text.into()],
            verification: vec!["human review required".into()],
            rollback: vec!["discard candidate".into()],
            do_not_use_when: vec!["not yet active".into()],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::TestReceipt,
        "test:candidate",
        Some("candidate".into()),
        json!({"exit_code":0}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault.capture(&memory, &[anchor], key).unwrap();
    memory.id
}

fn publish_knowledge(
    vault: &KnowledgeVault,
    agent: &AgentId,
    key: &str,
    title: &str,
    body: &str,
) -> String {
    publish_knowledge_version(vault, agent, key, title, body, &format!("proposal-{key}"))
}

fn publish_knowledge_version(
    vault: &KnowledgeVault,
    agent: &AgentId,
    key: &str,
    title: &str,
    body: &str,
    idempotency_key: &str,
) -> String {
    let source = ProposalSourceLock {
        source_type: ProposalSourceType::FileSnapshot,
        owner_agent_id: None,
        source_id: format!("reviewed-{key}"),
        source_revision: Some(1),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: None,
        approved_excerpt_hash: Some("b".repeat(64)),
        use_grant_id: None,
    };
    let proposal = vault
        .propose(
            agent,
            KnowledgeDraft::minimal(key, title, body),
            vec![source],
            idempotency_key,
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "reviewed retrieval fixture",
        )
        .unwrap();
    vault
        .publish(&proposal.id, proposal.revision, true)
        .unwrap()
        .edition_id
}

#[test]
fn chinese_exact_jieba_and_bigram_recall_private_memory() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("codex.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &vault,
        &agent,
        "Codex 文件句柄恢复",
        "远端 Codex MCP 文件句柄泄漏后发送 EOF 并验证子进程退出",
        "m1",
    );
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [2; 32]).unwrap();
    let engine = RecallEngine::new(vault, knowledge);
    for query in ["文件句柄泄漏", "Codex MCP", "子进程退出", "句柄恢复"] {
        let bundle = engine.recall(RecallRequest::new(query)).unwrap();
        assert_eq!(bundle.private_memories.len(), 1, "query={query}");
        assert_eq!(bundle.private_memories[0].origin, RecallOrigin::Memory);
    }
}

#[test]
fn bm25_rank_dominates_common_term_overlap() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [32; 32]).unwrap();
    for index in 0..12 {
        publish_knowledge(
            &knowledge,
            &agent,
            &format!("retrieval.background-{index}"),
            "common filler background",
            "common filler background material",
        );
    }
    let rare_id = publish_knowledge(
        &knowledge,
        &agent,
        "retrieval.rare",
        "rareterm",
        "rareterm recovery evidence",
    );
    publish_knowledge(
        &knowledge,
        &agent,
        "retrieval.common",
        "common filler",
        "common filler recovery evidence",
    );
    let engine = RecallEngine::new(private, knowledge);

    let bundle = engine
        .recall(RecallRequest::new("rareterm common filler"))
        .unwrap();

    assert_eq!(
        bundle.knowledge_editions[0].subject_ref,
        format!("knowledge:{rare_id}")
    );
    assert!(
        bundle.knowledge_editions[0]
            .match_reasons
            .contains(&"bm25_rank".into())
    );
}

#[test]
fn configurable_result_depth_returns_ten_with_large_budget() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [33; 32]).unwrap();
    for index in 0..12 {
        publish_knowledge(
            &knowledge,
            &agent,
            &format!("retrieval.depth-{index}"),
            &format!("shared marker {index}"),
            &format!("shared marker knowledge body {index}"),
        );
    }
    let engine = RecallEngine::new(private, knowledge);

    let default_bundle = engine.recall(RecallRequest::new("shared marker")).unwrap();
    assert_eq!(default_bundle.knowledge_editions.len(), 3);
    let expanded = engine
        .recall(RecallRequest {
            budget_tokens: 6_000,
            max_private_results: Some(10),
            max_knowledge_results: Some(10),
            ..RecallRequest::new("shared marker")
        })
        .unwrap();

    assert_eq!(expanded.knowledge_editions.len(), 10);
    assert_eq!(expanded.omitted_knowledge, 2);
}

#[test]
fn invalid_result_depth_fails_before_recall() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let engine = RecallEngine::new(
        AgentVault::open(root.path().join("agent.sqlite3"), agent).unwrap(),
        KnowledgeVault::open(root.path().join("knowledge"), [34; 32]).unwrap(),
    );

    for invalid in [0, 21] {
        let error = engine
            .recall(RecallRequest {
                max_knowledge_results: Some(invalid),
                ..RecallRequest::new("marker")
            })
            .unwrap_err();
        assert!(matches!(error, art_domain::ArtError::InvalidInput(_)));
    }
}

#[test]
fn private_and_published_channels_are_separate_and_cross_agent_memory_never_leaks() {
    let root = tempdir().unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    let codex_vault = AgentVault::open(root.path().join("codex.sqlite3"), codex.clone()).unwrap();
    let dsh_vault = AgentVault::open(root.path().join("dsh.sqlite3"), dsh.clone()).unwrap();
    seed_memory(
        &codex_vault,
        &codex,
        "Codex 私有恢复",
        "只有 Codex 应看到的彩虹恢复步骤",
        "c1",
    );
    seed_memory(
        &dsh_vault,
        &dsh,
        "DSH 私有恢复",
        "只有 DSH 应看到的月光恢复步骤",
        "d1",
    );
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [3; 32]).unwrap();
    let source = ProposalSourceLock {
        source_type: ProposalSourceType::FileSnapshot,
        owner_agent_id: None,
        source_id: "reviewed-doc".into(),
        source_revision: Some(1),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: None,
        approved_excerpt_hash: Some("b".repeat(64)),
        use_grant_id: None,
    };
    let proposal = knowledge
        .propose(
            &codex,
            KnowledgeDraft::minimal(
                "operations.shared-recovery",
                "共享恢复",
                "共享的星河恢复流程",
            ),
            vec![source],
            "k1",
        )
        .unwrap();
    knowledge
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    knowledge.publish(&proposal.id, 1, true).unwrap();

    let codex_engine = RecallEngine::new(codex_vault, knowledge.clone());
    let dsh_engine = RecallEngine::new(dsh_vault, knowledge);
    let codex_cross = codex_engine.recall(RecallRequest::new("月光恢复")).unwrap();
    let dsh_cross = dsh_engine.recall(RecallRequest::new("彩虹恢复")).unwrap();
    assert!(
        codex_cross
            .private_memories
            .iter()
            .all(|item| !item.title.contains("DSH") && !item.excerpt.contains("月光"))
    );
    assert!(
        dsh_cross
            .private_memories
            .iter()
            .all(|item| !item.title.contains("Codex") && !item.excerpt.contains("彩虹"))
    );
    assert_eq!(
        codex_engine
            .recall(RecallRequest::new("星河恢复"))
            .unwrap()
            .knowledge_editions
            .len(),
        1
    );
    assert_eq!(
        dsh_engine
            .recall(RecallRequest::new("星河恢复"))
            .unwrap()
            .knowledge_editions
            .len(),
        1
    );
}

#[test]
fn candidate_is_filtered_before_ranking_and_bundle_never_requests_automatic_capture() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let candidate = MemoryArtifact::new(
        agent.clone(),
        "高分候选",
        "完全匹配火箭火箭火箭",
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["x".into()],
            steps: vec!["火箭".into()],
            verification: vec!["x".into()],
            rollback: vec!["x".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        MemoryScope::User("local-user".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let anchor = SourceAnchor::new(
        agent,
        AnchorKind::FileSnapshot,
        "repo:file",
        Some("火箭".into()),
        json!({"digest":"x"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault.capture(&candidate, &[anchor], "candidate").unwrap();
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [4; 32]).unwrap(),
    );
    let filtered = engine.recall(RecallRequest::new("火箭")).unwrap();
    assert!(filtered.private_memories.is_empty());
    assert_eq!(filtered.persist_policy, "no_automatic_capture");
    let included = engine
        .recall(RecallRequest {
            include_candidates: true,
            ..RecallRequest::new("火箭")
        })
        .unwrap();
    assert_eq!(included.private_memories.len(), 1);
}

#[test]
fn sixty_four_chinese_golden_queries_keep_top_three_recall() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    for index in 0..16 {
        seed_memory(
            &vault,
            &agent,
            &format!("故障手册 {index}"),
            &format!(
                "故障代码 ART_E{index:02} 的中文恢复步骤，版本 v0.{index}.0，路径 src/module-{index}"
            ),
            &format!("g{index}"),
        );
    }
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [5; 32]).unwrap(),
    );
    for index in 0..16 {
        for query in [
            format!("ART_E{index:02}"),
            format!("module-{index}"),
            format!("v0.{index}.0"),
            format!("中文恢复步骤 {index}"),
        ] {
            let bundle = engine.recall(RecallRequest::new(query.clone())).unwrap();
            assert!(!bundle.private_memories.is_empty(), "query={query}");
            assert!(bundle.private_memories.len() <= 3);
        }
    }
}

#[test]
fn expired_and_disputed_memories_are_filtered_before_ranking() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let expired_id = seed_memory(
        &vault,
        &agent,
        "expired exact match",
        "ART_EXPIRED_EXACT_555",
        "expired",
    );
    let mut expired = vault.read(&expired_id).unwrap();
    expired.valid_until = Some(Utc::now() - Duration::seconds(1));
    let replacement_vault =
        AgentVault::open(root.path().join("expired.sqlite3"), agent.clone()).unwrap();
    replacement_vault
        .capture(
            &expired,
            &[SourceAnchor::new(
                agent.clone(),
                AnchorKind::TestReceipt,
                "test:expired",
                Some("expired".into()),
                json!({"exit_code":0}),
                Sensitivity::Private,
                Utc::now(),
            )
            .unwrap()],
            "expired-copy",
        )
        .unwrap();
    let disputed_id = seed_memory(
        &replacement_vault,
        &agent,
        "disputed exact match",
        "ART_DISPUTED_EXACT_556",
        "disputed",
    );
    replacement_vault
        .dispute(&disputed_id, "conflicting evidence")
        .unwrap();
    let engine = RecallEngine::new(
        replacement_vault,
        KnowledgeVault::open(root.path().join("knowledge"), [6; 32]).unwrap(),
    );
    for mode in [RetrievalMode::Lexical, RetrievalMode::FullScan] {
        assert!(
            engine
                .recall(RecallRequest {
                    mode,
                    ..RecallRequest::new("ART_EXPIRED_EXACT_555")
                })
                .unwrap()
                .private_memories
                .is_empty()
        );
        assert!(
            engine
                .recall(RecallRequest {
                    mode,
                    ..RecallRequest::new("ART_DISPUTED_EXACT_556")
                })
                .unwrap()
                .private_memories
                .is_empty()
        );
    }
}
