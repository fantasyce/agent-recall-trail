use std::{fs, str::FromStr};

use art_domain::{
    agent::{AgentId, ArtPaths},
    anchor::AnchorKind,
    memory::{MemoryPayload, ProcedurePayload, Sensitivity},
};
use art_mcp::{
    ArtMcpServer, FeedbackInput, GovernanceUiOpenInput, HealthInput, KnowledgeProposeInput,
    MemoryCandidateInput, MemoryCaptureInput, MemoryScopeType, ReadInput, RecallInput,
    SourceAnchorInput, governance_ui::UiView,
};
use art_retrieval::{RecallDetail, RetrievalMode};
use rmcp::handler::server::wrapper::Parameters;
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn verified_anchor(root: &std::path::Path) -> SourceAnchorInput {
    let path = root.join("verified-memory-evidence.txt");
    fs::write(&path, b"focused MCP memory fixture verified").unwrap();
    SourceAnchorInput {
        kind: AnchorKind::FileSnapshot,
        locator: path.display().to_string(),
        source_version: Some("1".into()),
        source_digest: Some(hex::encode(Sha256::digest(
            b"focused MCP memory fixture verified",
        ))),
        excerpt: None,
        metadata: json!({}),
    }
}

fn server() -> (tempfile::TempDir, ArtMcpServer) {
    let root = tempdir().unwrap();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let server = ArtMcpServer::open(&paths, agent, [4; 32]).unwrap();
    (root, server)
}

#[tokio::test]
async fn mcp_discovers_optional_embedding_without_changing_the_nine_tool_surface() {
    let root = tempdir().unwrap();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let config_dir = root.path().join("config/art/embedding");
    fs::create_dir_all(&config_dir).unwrap();
    let config = config_dir.join("default.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({
            "schema":"art.embedding.endpoint.v1",
            "protocol":"openai_compatible",
            "endpoint":"https://embedding.example.test",
            "model":"operator/model",
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
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let server = ArtMcpServer::open(
        &paths,
        AgentId::from_str("codex-primary").unwrap(),
        [46; 32],
    )
    .unwrap();
    assert_eq!(server.tool_names().len(), 9);
    let health = server.art_health(Parameters(HealthInput {})).await.unwrap();
    assert_eq!(health.0.fields["vector_status"], "stale");
}

#[test]
fn tool_surface_is_exactly_nine_agent_safe_tools() {
    let (_root, server) = server();
    let names = server.tool_names();
    assert_eq!(
        names,
        vec![
            "art_feedback",
            "art_governance_ui_open",
            "art_health",
            "art_knowledge_governance",
            "art_knowledge_propose",
            "art_memory_candidate_submit",
            "art_memory_capture",
            "art_read",
            "art_recall",
        ]
    );
    for forbidden in [
        "approve",
        "publish",
        "delete",
        "grant",
        "other_agent",
        "sql",
    ] {
        assert!(names.iter().all(|name| !name.contains(forbidden)));
    }
    let schemas = server.tool_schema_json();
    assert!(!schemas.contains("owner_agent_id"));
    assert!(!schemas.contains("target_agent"));
    let tools: serde_json::Value = serde_json::from_str(&schemas).unwrap();
    for tool in tools.as_array().unwrap() {
        assert_eq!(tool["outputSchema"]["type"], "object");
    }
    let governance = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "art_knowledge_governance")
        .unwrap();
    let properties = &governance["inputSchema"]["properties"];
    for allowed in ["operation", "proposal_id", "revision"] {
        assert!(properties.get(allowed).is_some(), "missing {allowed}");
    }
    for forbidden in ["decision", "reason", "actor", "confirm"] {
        assert!(properties.get(forbidden).is_none(), "exposed {forbidden}");
    }
    assert_eq!(
        governance["inputSchema"]["additionalProperties"], false,
        "Agent governance input must reject undeclared authority fields"
    );
    let candidate = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "art_memory_candidate_submit")
        .unwrap();
    for forbidden in ["owner_agent_id", "status", "actor", "reviewer", "assurance"] {
        assert!(
            candidate["inputSchema"]["properties"]
                .get(forbidden)
                .is_none(),
            "candidate tool exposed {forbidden}"
        );
    }
    assert_eq!(
        candidate["inputSchema"]["properties"]["candidate_index"]["maximum"],
        0
    );
    assert_eq!(
        candidate["inputSchema"]["properties"]["scope_type"]["$ref"],
        "#/$defs/MemoryScopeType"
    );
    assert_eq!(
        candidate["inputSchema"]["$defs"]["MemoryScopeType"]["enum"],
        json!(["session", "repository", "workspace", "machine", "user"]),
        "candidate scope schema must prevent the host Agent from inventing unsupported scope names"
    );
}

#[tokio::test]
async fn automatic_candidate_tool_is_default_off_then_uses_bound_agent_and_policy() {
    let (root, server) = server();
    let evidence_path = root.path().join("mcp-candidate-evidence.txt");
    fs::write(&evidence_path, b"focused candidate contract passed").unwrap();
    let evidence_digest = hex::encode(Sha256::digest(b"focused candidate contract passed"));
    let input = MemoryCandidateInput {
        trigger_receipt_id: "artamt_fixture".into(),
        candidate_index: 0,
        title: "Verified MCP candidate".into(),
        summary: "The bounded MCP candidate passed its focused verification.".into(),
        payload: MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["Use the current ART candidate runtime.".into()],
            steps: vec!["Submit through the bound candidate tool.".into()],
            verification: vec!["Run the focused MCP contract tests.".into()],
            rollback: vec!["Disable global automatic memory.".into()],
            do_not_use_when: vec!["The source receipt is stale.".into()],
        }),
        scope_type: MemoryScopeType::Repository,
        scope_key: "agent-recall-trail".into(),
        sensitivity: Sensitivity::Internal,
        anchors: vec![SourceAnchorInput {
            kind: AnchorKind::FileSnapshot,
            locator: evidence_path.display().to_string(),
            source_version: Some("focused-1".into()),
            source_digest: Some(evidence_digest),
            excerpt: None,
            metadata: json!({"evidence_scope":"art-mcp candidate contract"}),
        }],
    };
    let disabled = server
        .art_memory_candidate_submit(Parameters(input.clone()))
        .await
        .unwrap();
    assert_eq!(disabled.0.fields["disposition"], "disabled");

    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let mut input = input;
    input.trigger_receipt_id = "different-forged-trigger".into();
    let Err(forged) = server
        .art_memory_candidate_submit(Parameters(input.clone()))
        .await
    else {
        panic!("forged trigger receipt unexpectedly accepted");
    };
    assert!(forged.contains("ART_PERMISSION_DENIED"));
    let vault = art_agent_store::AgentVault::open(
        root.path()
            .join("data/art/agents/codex-primary/art.sqlite3"),
        AgentId::from_str("codex-primary").unwrap(),
    )
    .unwrap();
    let event_hash = hex::encode(Sha256::digest(b"mcp-candidate-trigger"));
    let trigger = vault
        .claim_auto_memory_trigger(
            &art_agent_store::AutoMemoryConfigStore::new(root.path()),
            "mcp-session",
            "mcp-turn",
            &event_hash,
            chrono::Utc::now(),
        )
        .unwrap();
    let mut input = input;
    input.trigger_receipt_id = trigger.receipt_id;
    let result = server
        .art_memory_candidate_submit(Parameters(input))
        .await
        .unwrap();
    assert_eq!(result.0.fields["agent_id"], "codex-primary");
    assert_eq!(result.0.fields["outcome"], "activated");
    assert_eq!(result.0.fields["status"], "active");
    assert_eq!(result.0.fields["policy_actor"], "system_policy");
    let memory_id = result.0.fields["memory_id"].as_str().unwrap();
    let recalled = server
        .art_recall(Parameters(RecallInput {
            query: "bounded candidate focused verification".into(),
            mode: RetrievalMode::Lexical,
            detail: RecallDetail::Recall,
            include_candidates: false,
            budget_tokens: 1_800,
            max_private_results: None,
            max_knowledge_results: None,
        }))
        .await
        .unwrap();
    assert_eq!(
        recalled.0.fields["private_memories"][0]["subject_ref"],
        format!("memory:{memory_id}@1")
    );
    let exact = server
        .art_read(Parameters(ReadInput {
            subject_ref: format!("memory:{memory_id}@1"),
            include_anchors: true,
        }))
        .await
        .unwrap();
    assert_eq!(exact.0.fields["id"], memory_id);
    assert_eq!(exact.0.fields["status"], "active");

    let health = server.art_health(Parameters(HealthInput {})).await.unwrap();
    assert_eq!(health.0.fields["auto_memory"]["enabled"], true);
    assert_eq!(
        health.0.fields["auto_memory"]["policy_version"],
        "art.auto-memory.policy.v1"
    );
}

#[test]
fn anchor_kind_is_an_exact_enum_in_the_memory_capture_schema() {
    let (_root, server) = server();
    let tools: serde_json::Value = serde_json::from_str(&server.tool_schema_json()).unwrap();
    let capture = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "art_memory_capture")
        .unwrap();
    let anchor = &capture["inputSchema"]["$defs"]["SourceAnchorInput"];
    let kind = &anchor["properties"]["kind"];
    let kind_schema = kind.get("enum").map_or_else(
        || {
            let pointer = kind["$ref"]
                .as_str()
                .and_then(|reference| reference.strip_prefix('#'))
                .expect("anchor kind must be an inline or referenced enum");
            capture["inputSchema"]
                .pointer(pointer)
                .expect("anchor kind reference must resolve inside inputSchema")
        },
        |_| kind,
    );
    assert_eq!(
        kind_schema["enum"],
        json!([
            "host_session_range",
            "user_statement",
            "file_snapshot",
            "git_object",
            "command_receipt",
            "test_receipt",
            "log_excerpt",
            "external_document"
        ])
    );
}

#[test]
fn invalid_anchor_kind_reports_canonical_alternatives() {
    let error = serde_json::from_value::<MemoryCaptureInput>(json!({
        "title": "Invalid anchor vocabulary",
        "summary": "The request must fail before persistence.",
        "payload": {
            "kind": "semantic",
            "data": {
                "statement": "invalid",
                "applicability": "schema regression",
                "exceptions": []
            }
        },
        "scope_type": "repository",
        "scope_key": "agent-recall-trail",
        "sensitivity": "private",
        "idempotency_key": "invalid-anchor-kind",
        "anchors": [{
            "kind": "git",
            "locator": "commit:deadbeef"
        }]
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("unknown variant `git`"), "{error}");
    assert!(error.contains("git_object"), "{error}");
    assert!(error.contains("external_document"), "{error}");
}

#[tokio::test]
async fn governance_ui_open_returns_a_bounded_loopback_session() {
    let (_root, server) = server();
    let result = server
        .art_governance_ui_open(Parameters(GovernanceUiOpenInput {
            view: UiView::Settings,
            proposal_id: None,
            revision: None,
        }))
        .await
        .unwrap();
    let url = result.0.fields["url"].as_str().unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));
    assert_eq!(result.0.fields["view"], "settings");
    assert!(!result.0.fields.contains_key("capability"));
}

#[test]
fn recall_result_depth_is_optional_and_bounded_in_the_tool_schema() {
    let (_root, server) = server();
    let tools: serde_json::Value = serde_json::from_str(&server.tool_schema_json()).unwrap();
    let recall = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "art_recall")
        .unwrap();
    for field in ["max_private_results", "max_knowledge_results"] {
        let property = &recall["inputSchema"]["properties"][field];
        assert_eq!(property["minimum"], 1);
        assert_eq!(property["maximum"], 20);
        assert!(
            !recall["inputSchema"]["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|value| value == field))
        );
    }
    let properties = &recall["inputSchema"]["properties"];
    let enum_values = |property: &serde_json::Value| {
        property.get("enum").cloned().unwrap_or_else(|| {
            let pointer = property["$ref"]
                .as_str()
                .and_then(|reference| reference.strip_prefix('#'))
                .expect("enum property must be inline or reference a local schema");
            recall["inputSchema"]
                .pointer(pointer)
                .and_then(|definition| definition.get("enum"))
                .cloned()
                .expect("referenced schema must define enum values")
        })
    };
    assert_eq!(
        enum_values(&properties["mode"]),
        json!(["lexical", "full_scan", "semantic", "hybrid"])
    );
    assert_eq!(
        enum_values(&properties["detail"]),
        json!(["route", "recall"])
    );
}

#[tokio::test]
async fn recall_result_depth_is_forwarded_to_validation() {
    let (_root, server) = server();
    let result = server
        .art_recall(Parameters(RecallInput {
            query: "marker".into(),
            mode: RetrievalMode::Lexical,
            detail: RecallDetail::Recall,
            include_candidates: false,
            budget_tokens: 1_800,
            max_private_results: None,
            max_knowledge_results: Some(21),
        }))
        .await;

    let Err(error) = result else {
        panic!("invalid result depth unexpectedly succeeded");
    };
    assert!(error.contains("ART_INVALID_INPUT"));
}

#[tokio::test]
async fn capture_then_recall_stays_bound_to_process_identity() {
    let (root, server) = server();
    let input = MemoryCaptureInput {
        capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
        request_basis: Some("User explicitly requested this fixture memory".into()),
        value_reason: None,
        session_id: None,
        turn_id: None,
        memory_id: None,
        expected_revision: None,
        title: "ART MCP shutdown".into(),
        summary: "stdin EOF 后三秒内关闭子进程".into(),
        payload: MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["确认父进程".into()],
            steps: vec!["发送 EOF".into()],
            verification: vec!["进程退出".into()],
            rollback: vec!["重开会话".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        scope_type: MemoryScopeType::Repository,
        scope_key: "agent-recall-trail".into(),
        sensitivity: Sensitivity::Private,
        idempotency_key: "mcp-capture-1".into(),
        anchors: vec![verified_anchor(root.path())],
        unanchored_candidate: false,
        no_persist_provenance: false,
    };
    let captured = server
        .art_memory_capture(Parameters(input.clone()))
        .await
        .unwrap();
    let replay = server.art_memory_capture(Parameters(input)).await.unwrap();
    assert_eq!(captured.0.fields["memory_id"], replay.0.fields["memory_id"]);
    assert!(
        captured.0.fields["memory_id"]
            .as_str()
            .unwrap()
            .starts_with("artm_")
    );
    let recalled = server
        .art_recall(Parameters(RecallInput {
            query: "EOF 关闭子进程".into(),
            mode: RetrievalMode::Lexical,
            detail: RecallDetail::Recall,
            include_candidates: false,
            budget_tokens: 1800,
            max_private_results: None,
            max_knowledge_results: None,
        }))
        .await
        .unwrap();
    assert_eq!(recalled.0.fields["agent_id"], "codex-primary");
    assert_eq!(
        recalled.0.fields["private_memories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn exact_expected_revision_updates_atomically_and_replays_idempotently() {
    let (root, server) = server();
    let original = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
            request_basis: Some("User explicitly requested this fixture memory".into()),
            value_reason: None,
            session_id: None,
            turn_id: None,
            memory_id: None,
            expected_revision: None,
            title: "Revision one".into(),
            summary: "first claim".into(),
            payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
                statement: "first".into(),
                applicability: "revision test".into(),
                exceptions: vec![],
            }),
            scope_type: MemoryScopeType::User,
            scope_key: "*".into(),
            sensitivity: Sensitivity::Private,
            idempotency_key: "revision-original".into(),
            anchors: vec![verified_anchor(root.path())],
            unanchored_candidate: false,
            no_persist_provenance: false,
        }))
        .await
        .unwrap();
    let memory_id = original.0.fields["memory_id"].as_str().unwrap().to_owned();
    let revision = MemoryCaptureInput {
        capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
        request_basis: Some("User explicitly requested this fixture memory".into()),
        value_reason: None,
        session_id: None,
        turn_id: None,
        memory_id: Some(memory_id.clone()),
        expected_revision: Some(1),
        title: "Revision two".into(),
        summary: "second claim".into(),
        payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
            statement: "second".into(),
            applicability: "revision test".into(),
            exceptions: vec![],
        }),
        scope_type: MemoryScopeType::User,
        scope_key: "*".into(),
        sensitivity: Sensitivity::Private,
        idempotency_key: "revision-update".into(),
        anchors: vec![verified_anchor(root.path())],
        unanchored_candidate: false,
        no_persist_provenance: false,
    };
    let updated = server
        .art_memory_capture(Parameters(revision.clone()))
        .await
        .unwrap();
    let replay = server
        .art_memory_capture(Parameters(revision))
        .await
        .unwrap();
    assert_eq!(updated.0.fields["revision"], 2);
    assert_eq!(replay.0.fields["revision"], 2);
    let stale = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
            request_basis: Some("User explicitly requested this fixture memory".into()),
            value_reason: None,
            session_id: None,
            turn_id: None,
            memory_id: Some(memory_id),
            expected_revision: Some(1),
            title: "Revision conflict".into(),
            summary: "conflict".into(),
            payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
                statement: "conflict".into(),
                applicability: "revision test".into(),
                exceptions: vec![],
            }),
            scope_type: MemoryScopeType::User,
            scope_key: "*".into(),
            sensitivity: Sensitivity::Private,
            idempotency_key: "revision-stale".into(),
            anchors: vec![verified_anchor(root.path())],
            unanchored_candidate: false,
            no_persist_provenance: false,
        }))
        .await;
    let Err(stale) = stale else {
        panic!("stale revision unexpectedly succeeded");
    };
    assert!(stale.contains("ART_SOURCE_STALE"));
}

#[tokio::test]
async fn no_persist_provenance_is_rejected_with_stable_code() {
    let (_root, server) = server();
    let result = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
            request_basis: Some("User explicitly requested this fixture memory".into()),
            value_reason: None,
            session_id: None,
            turn_id: None,
            memory_id: None,
            expected_revision: None,
            title: "forbidden".into(),
            summary: "grant excerpt".into(),
            payload: MemoryPayload::Procedure(ProcedurePayload {
                prerequisites: vec!["x".into()],
                steps: vec!["x".into()],
                verification: vec!["x".into()],
                rollback: vec!["x".into()],
                do_not_use_when: vec!["outside the documented scope".into()],
            }),
            scope_type: MemoryScopeType::User,
            scope_key: "*".into(),
            sensitivity: Sensitivity::Private,
            idempotency_key: "blocked".into(),
            anchors: vec![],
            unanchored_candidate: true,
            no_persist_provenance: true,
        }))
        .await;
    let Err(error) = result else {
        panic!("no-persist capture unexpectedly succeeded");
    };
    assert!(error.contains("ART_NO_PERSIST"));
}

#[tokio::test]
async fn feedback_idempotency_replays_and_conflicting_payload_is_rejected() {
    let (_root, server) = server();
    let input = FeedbackInput {
        subject_ref: "memory:artm_missing".into(),
        signal: "stale".into(),
        safe_note: Some("verify again".into()),
        idempotency_key: "feedback-1".into(),
    };
    let first = server
        .art_feedback(Parameters(input.clone()))
        .await
        .unwrap();
    let replay = server.art_feedback(Parameters(input)).await.unwrap();
    assert_eq!(
        first.0.fields["feedback_id"],
        replay.0.fields["feedback_id"]
    );
    let conflict = server
        .art_feedback(Parameters(FeedbackInput {
            subject_ref: "memory:artm_missing".into(),
            signal: "unsafe".into(),
            safe_note: Some("different".into()),
            idempotency_key: "feedback-1".into(),
        }))
        .await;
    let Err(error) = conflict else {
        panic!("conflicting feedback unexpectedly succeeded");
    };
    assert!(error.contains("ART_DUPLICATE_CONFLICT"));
}

#[tokio::test]
async fn original_six_agent_safe_tools_keep_success_paths_and_stale_reads_fail_closed() {
    let (root, server) = server();
    let captured = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            capture_origin: Some(art_mcp::CaptureOrigin::UserRequested),
            request_basis: Some("User explicitly requested this fixture memory".into()),
            value_reason: None,
            session_id: None,
            turn_id: None,
            memory_id: None,
            expected_revision: None,
            title: "Original six tools".into(),
            summary: "六个工具都必须通过真实调用".into(),
            payload: MemoryPayload::Procedure(ProcedurePayload {
                prerequisites: vec!["ART 已初始化".into()],
                steps: vec!["逐个调用".into()],
                verification: vec!["检查结构化结果".into()],
                rollback: vec!["不发布".into()],
                do_not_use_when: vec!["outside the documented scope".into()],
            }),
            scope_type: MemoryScopeType::Repository,
            scope_key: "agent-recall-trail".into(),
            sensitivity: Sensitivity::Internal,
            idempotency_key: "all-tools-capture".into(),
            anchors: vec![verified_anchor(root.path())],
            unanchored_candidate: false,
            no_persist_provenance: false,
        }))
        .await
        .unwrap();
    let memory_id = captured.0.fields["memory_id"].as_str().unwrap();
    let read = server
        .art_read(Parameters(ReadInput {
            subject_ref: format!("memory:{memory_id}@1"),
            include_anchors: false,
        }))
        .await
        .unwrap();
    assert_eq!(read.0.fields["id"], memory_id);
    let stale = server
        .art_read(Parameters(ReadInput {
            subject_ref: format!("memory:{memory_id}@2"),
            include_anchors: false,
        }))
        .await;
    let Err(stale) = stale else {
        panic!("stale revision unexpectedly returned content");
    };
    assert!(stale.contains("ART_NOT_FOUND"));
    let proposed = server
        .art_knowledge_propose(Parameters(KnowledgeProposeInput {
            knowledge_key: "mcp.original-six-tools".into(),
            title: "Original six-tool contract".into(),
            applicability: "MCP conformance".into(),
            markdown: "The original six tools were invoked through the bound server.".into(),
            sensitivity: Sensitivity::Internal,
            source_refs: vec![format!("memory:{memory_id}@1")],
            idempotency_key: "all-tools-proposal".into(),
        }))
        .await
        .unwrap();
    assert_eq!(proposed.0.fields["status"], "submitted");
    server
        .art_feedback(Parameters(FeedbackInput {
            subject_ref: format!("memory:{memory_id}"),
            signal: "relevant".into(),
            safe_note: None,
            idempotency_key: "all-tools-feedback".into(),
        }))
        .await
        .unwrap();
    let health = server.art_health(Parameters(HealthInput {})).await.unwrap();
    assert_eq!(health.0.fields["bound_agent_id"], "codex-primary");
    assert_eq!(health.0.fields["map_status"], "ready");
    assert_eq!(health.0.fields["private_navigation_aligned"], true);
    assert_eq!(health.0.fields["knowledge_navigation_aligned"], true);
}

#[tokio::test]
async fn database_lock_is_reported_with_the_stable_retryable_code() {
    let (root, server) = server();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let lock = Connection::open(paths.agent_vault(&agent)).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let result = server
        .art_feedback(Parameters(FeedbackInput {
            subject_ref: "memory:artm_locked".into(),
            signal: "stale".into(),
            safe_note: None,
            idempotency_key: "locked-feedback".into(),
        }))
        .await;
    lock.execute_batch("ROLLBACK").unwrap();
    let Err(error) = result else {
        panic!("locked database unexpectedly accepted a write");
    };
    assert!(error.contains("ART_DB_BUSY"));
    assert!(error.contains("\"retryable\":true"));
}

#[tokio::test]
async fn new_requests_are_rejected_after_shutdown_begins() {
    let (_root, server) = server();
    server.test_only_begin_shutdown();
    let result = server.art_health(Parameters(HealthInput {})).await;
    let Err(error) = result else {
        panic!("request unexpectedly succeeded after shutdown began");
    };
    assert!(error.contains("ART_SHUTTING_DOWN"));
    assert!(error.contains("\"retryable\":true"));
}
