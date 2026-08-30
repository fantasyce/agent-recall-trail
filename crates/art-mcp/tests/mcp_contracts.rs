use std::str::FromStr;

use art_domain::{
    agent::{AgentId, ArtPaths},
    memory::{MemoryPayload, ProcedurePayload, Sensitivity},
};
use art_mcp::{
    ArtMcpServer, FeedbackInput, HealthInput, KnowledgeProposeInput, MemoryCaptureInput, ReadInput,
    RecallInput, SourceAnchorInput,
};
use rmcp::handler::server::wrapper::Parameters;
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn server() -> (tempfile::TempDir, ArtMcpServer) {
    let root = tempdir().unwrap();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let server = ArtMcpServer::open(&paths, agent, [4; 32]).unwrap();
    (root, server)
}

#[test]
fn tool_surface_is_exactly_six_agent_safe_tools() {
    let (_root, server) = server();
    let names = server.tool_names();
    assert_eq!(
        names,
        vec![
            "art_feedback",
            "art_health",
            "art_knowledge_propose",
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
}

#[tokio::test]
async fn recall_result_depth_is_forwarded_to_validation() {
    let (_root, server) = server();
    let result = server
        .art_recall(Parameters(RecallInput {
            query: "marker".into(),
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
    let (_root, server) = server();
    let input = MemoryCaptureInput {
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
        scope_type: "repository".into(),
        scope_key: "agent-recall-trail".into(),
        sensitivity: Sensitivity::Private,
        idempotency_key: "mcp-capture-1".into(),
        anchors: vec![SourceAnchorInput {
            kind: "test_receipt".into(),
            locator: "test:mcp-eof".into(),
            source_version: Some("1".into()),
            source_digest: Some("sha256:abc".into()),
            excerpt: Some("exit code 0".into()),
            metadata: json!({"exit_code":0,"output_hash":"abc"}),
        }],
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
    let (_root, server) = server();
    let original = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            memory_id: None,
            expected_revision: None,
            title: "Revision one".into(),
            summary: "first claim".into(),
            payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
                statement: "first".into(),
                applicability: "revision test".into(),
                exceptions: vec![],
            }),
            scope_type: "user".into(),
            scope_key: "*".into(),
            sensitivity: Sensitivity::Private,
            idempotency_key: "revision-original".into(),
            anchors: vec![SourceAnchorInput {
                kind: "user_statement".into(),
                locator: "test:revision-one".into(),
                source_version: None,
                source_digest: None,
                excerpt: Some("first".into()),
                metadata: json!({}),
            }],
            unanchored_candidate: false,
            no_persist_provenance: false,
        }))
        .await
        .unwrap();
    let memory_id = original.0.fields["memory_id"].as_str().unwrap().to_owned();
    let revision = MemoryCaptureInput {
        memory_id: Some(memory_id.clone()),
        expected_revision: Some(1),
        title: "Revision two".into(),
        summary: "second claim".into(),
        payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
            statement: "second".into(),
            applicability: "revision test".into(),
            exceptions: vec![],
        }),
        scope_type: "user".into(),
        scope_key: "*".into(),
        sensitivity: Sensitivity::Private,
        idempotency_key: "revision-update".into(),
        anchors: vec![SourceAnchorInput {
            kind: "user_statement".into(),
            locator: "test:revision-two".into(),
            source_version: None,
            source_digest: None,
            excerpt: Some("second".into()),
            metadata: json!({}),
        }],
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
            memory_id: Some(memory_id),
            expected_revision: Some(1),
            title: "Revision conflict".into(),
            summary: "conflict".into(),
            payload: MemoryPayload::Semantic(art_domain::memory::SemanticPayload {
                statement: "conflict".into(),
                applicability: "revision test".into(),
                exceptions: vec![],
            }),
            scope_type: "user".into(),
            scope_key: "*".into(),
            sensitivity: Sensitivity::Private,
            idempotency_key: "revision-stale".into(),
            anchors: vec![SourceAnchorInput {
                kind: "user_statement".into(),
                locator: "test:revision-stale".into(),
                source_version: None,
                source_digest: None,
                excerpt: Some("conflict".into()),
                metadata: json!({}),
            }],
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
            scope_type: "user".into(),
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
async fn every_agent_safe_tool_has_a_success_path_and_stale_reads_fail_closed() {
    let (_root, server) = server();
    let captured = server
        .art_memory_capture(Parameters(MemoryCaptureInput {
            memory_id: None,
            expected_revision: None,
            title: "Six tools".into(),
            summary: "六个工具都必须通过真实调用".into(),
            payload: MemoryPayload::Procedure(ProcedurePayload {
                prerequisites: vec!["ART 已初始化".into()],
                steps: vec!["逐个调用".into()],
                verification: vec!["检查结构化结果".into()],
                rollback: vec!["不发布".into()],
                do_not_use_when: vec!["outside the documented scope".into()],
            }),
            scope_type: "repository".into(),
            scope_key: "agent-recall-trail".into(),
            sensitivity: Sensitivity::Internal,
            idempotency_key: "all-tools-capture".into(),
            anchors: vec![SourceAnchorInput {
                kind: "test_receipt".into(),
                locator: "test:all-tools".into(),
                source_version: None,
                source_digest: Some("sha256:all-tools".into()),
                excerpt: Some("six tool contract".into()),
                metadata: json!({"exit_code":0,"output_hash":"all-tools"}),
            }],
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
            knowledge_key: "mcp.six-tools".into(),
            title: "Six tool contract".into(),
            applicability: "MCP conformance".into(),
            markdown: "All six tools were invoked through the bound server.".into(),
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
