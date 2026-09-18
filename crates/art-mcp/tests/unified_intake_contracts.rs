use art_agent_store::{AgentVault, AutoMemoryConfigStore};
use art_domain::{
    agent::{AgentId, ArtPaths},
    anchor::SourceAnchor,
    memory::{MemoryArtifact, MemoryScope, MemoryStatus},
};
use art_mcp::{ArtMcpServer, MemoryCandidateInput, MemoryCaptureInput};
use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

fn setup() -> (TempDir, ArtPaths, ArtMcpServer, AgentVault) {
    let root = tempdir().unwrap();
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let agent: AgentId = "codex-primary".parse().unwrap();
    let server = ArtMcpServer::open(&paths, agent.clone(), [4; 32]).unwrap();
    let vault = AgentVault::open(paths.agent_vault(&agent), agent).unwrap();
    (root, paths, server, vault)
}

fn capture(key: &str) -> Value {
    json!({"title":key,"summary":"A durable user preference for future work",
        "payload":{"kind":"semantic","data":{"statement":key,"applicability":"user preferences","exceptions":[]}},
        "scope_type":"user","scope_key":"*","sensitivity":"internal","idempotency_key":key,
        "value_reason":"Reapply the stable preference in future tasks",
        "anchors":[{"kind":"user_statement","locator":"user:preference","excerpt":"Use focused checks","metadata":{}}]})
}

async fn call(server: &ArtMcpServer, value: Value) -> Result<Value, String> {
    let input: MemoryCaptureInput = serde_json::from_value(value).unwrap();
    server
        .art_memory_capture(Parameters(input))
        .await
        .map(|out| serde_json::to_value(out.0).unwrap())
}

#[tokio::test]
async fn omitted_origin_is_automatic_and_disabled_returns_durable_unified_receipt() {
    let (_root, _paths, server, vault) = setup();
    let input = capture("disabled-default");
    let first = call(&server, input.clone()).await.unwrap();
    assert_eq!(first["disposition"], "disabled");
    assert_eq!(first["origin"], "agent_initiated");
    assert!(first["memory_id"].is_null());
    assert_eq!(first["receipt"]["attribution"]["degraded"], true);
    assert!(vault.list().unwrap().is_empty());
    let replay = call(&server, input).await.unwrap();
    assert_eq!(replay["receipt_id"], first["receipt_id"]);
    assert_eq!(replay["replayed"], true);
}

#[tokio::test]
async fn origins_require_their_bounded_explanations_and_forbid_identity_injection() {
    let (_root, _paths, server, _) = setup();
    for origin in ["user_requested", "agent_initiated"] {
        let mut input = capture(origin);
        input["capture_origin"] = json!(origin);
        input.as_object_mut().unwrap().remove("value_reason");
        assert!(
            call(&server, input)
                .await
                .unwrap_err()
                .contains("ART_INVALID_INPUT")
        );
    }
    for field in [
        "host_supplied",
        "agent_id",
        "status",
        "assurance",
        "reviewer",
        "policy_actor",
    ] {
        let mut input = capture("forged");
        input[field] = json!(true);
        assert!(
            serde_json::from_value::<MemoryCaptureInput>(input).is_err(),
            "accepted {field}"
        );
    }
    for origin in ["hook_triggered", "legacy_unspecified"] {
        let mut input = capture("forged-origin");
        input["capture_origin"] = json!(origin);
        assert!(serde_json::from_value::<MemoryCaptureInput>(input).is_err());
    }
    let mut empty_scope = capture("fresh-empty-scope");
    empty_scope["scope_key"] = json!("");
    assert!(
        call(&server, empty_scope)
            .await
            .unwrap_err()
            .contains("ART_INVALID_INPUT")
    );
}

#[tokio::test]
async fn explicit_request_works_disabled_and_automatic_works_without_hook_using_fallback_budget() {
    let (root, paths, server, vault) = setup();
    let mut explicit = capture("explicit");
    explicit["capture_origin"] = json!("user_requested");
    explicit["request_basis"] = json!("User asked to remember this preference");
    let result = call(&server, explicit).await.unwrap();
    assert_eq!(result["origin"], "user_requested");
    assert_eq!(result["disposition"], "pending_review");
    AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let mut automatic = capture("automatic");
    automatic["session_id"] = json!("claimed-session");
    automatic["turn_id"] = json!("claimed-turn");
    let result = call(&server, automatic).await.unwrap();
    assert_eq!(result["disposition"], "pending_review");
    assert_eq!(result["receipt"]["attribution"]["host_supplied"], false);
    assert_eq!(
        result["receipt"]["attribution"]["session_id"],
        "claimed-session"
    );
    assert!(
        result["receipt"]["budget_bucket"]
            .as_str()
            .unwrap()
            .starts_with("agent-day:")
    );
    drop(server);
    let server = ArtMcpServer::open(&paths, "codex-primary".parse().unwrap(), [4; 32]).unwrap();
    let result = call(&server, capture("next automatic")).await.unwrap();
    assert_eq!(result["disposition"], "rate_limited");
    assert!(result["memory_id"].is_null());
    assert_eq!(vault.list().unwrap().len(), 2);
}

#[tokio::test]
async fn hook_adapter_requires_exact_receipt_and_returns_same_unified_contract() {
    let (root, _, server, vault) = setup();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let trigger = vault
        .claim_auto_memory_trigger(
            &config,
            "host-session",
            "host-turn",
            &"a".repeat(64),
            Utc::now(),
        )
        .unwrap();
    let mut value = capture("hook");
    value.as_object_mut().unwrap().remove("idempotency_key");
    value.as_object_mut().unwrap().remove("value_reason");
    value["candidate_index"] = json!(0);
    value["trigger_receipt_id"] = json!("forged");
    let input: MemoryCandidateInput = serde_json::from_value(value.clone()).unwrap();
    assert!(
        server
            .art_memory_candidate_submit(Parameters(input))
            .await
            .err()
            .unwrap()
            .contains("ART_PERMISSION_DENIED")
    );
    value["trigger_receipt_id"] = json!(trigger.receipt_id);
    let input: MemoryCandidateInput = serde_json::from_value(value).unwrap();
    let result = server
        .art_memory_candidate_submit(Parameters(input.clone()))
        .await
        .unwrap()
        .0
        .fields;
    assert_eq!(result["origin"], "hook_triggered");
    assert_eq!(result["disposition"], "pending_review");
    assert_eq!(
        result["receipt"]["attribution"]["session_id"],
        "host-session"
    );
    assert_eq!(result["receipt"]["attribution"]["host_supplied"], true);
    let replay = server
        .art_memory_candidate_submit(Parameters(input))
        .await
        .unwrap()
        .0
        .fields;
    assert_eq!(replay["receipt_id"], result["receipt_id"]);
    assert_eq!(replay["replayed"], true);
}

#[tokio::test]
async fn historical_mcp_capture_and_revision_retries_keep_original_receipts() {
    let (_root, paths, server, vault) = setup();
    for unanchored in [false, true] {
        let mut value = capture(if unanchored {
            "old-unanchored"
        } else {
            "old-active"
        });
        value.as_object_mut().unwrap().remove("value_reason");
        if unanchored {
            value["anchors"] = json!([]);
            value["unanchored_candidate"] = json!(true);
        }
        let input: MemoryCaptureInput = serde_json::from_value(value.clone()).unwrap();
        let anchors: Vec<_> = input
            .anchors
            .iter()
            .map(|a| {
                SourceAnchor::new_with_source(
                    vault.agent_id().clone(),
                    a.kind,
                    a.locator.clone(),
                    a.source_version.clone(),
                    a.source_digest.clone(),
                    a.excerpt.clone(),
                    a.metadata.clone(),
                    input.sensitivity,
                    Utc::now(),
                )
                .unwrap()
            })
            .collect();
        let mut memory = MemoryArtifact::new(
            vault.agent_id().clone(),
            input.title.clone(),
            input.summary.clone(),
            input.payload.clone(),
            MemoryScope::User("*".into()),
            input.sensitivity,
            Utc::now(),
        )
        .unwrap();
        if !unanchored {
            memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
        }
        vault
            .capture(&memory, &anchors, &input.idempotency_key)
            .unwrap();
        let reopened = ArtMcpServer::open(&paths, vault.agent_id().clone(), [4; 32]).unwrap();
        let replay = call(&reopened, value.clone()).await.unwrap();
        assert_eq!(replay["origin"], "legacy_unspecified");
        assert_eq!(replay["memory_id"], memory.id);
        if !unanchored {
            let mut revised = input.payload.clone();
            if let art_domain::memory::MemoryPayload::Semantic(p) = &mut revised {
                p.statement = "updated preference".into();
            }
            let revision_anchors: Vec<_> = input
                .anchors
                .iter()
                .map(|a| {
                    SourceAnchor::new_with_source(
                        vault.agent_id().clone(),
                        a.kind,
                        a.locator.clone(),
                        a.source_version.clone(),
                        a.source_digest.clone(),
                        a.excerpt.clone(),
                        a.metadata.clone(),
                        input.sensitivity,
                        Utc::now(),
                    )
                    .unwrap()
                })
                .collect();
            vault
                .revise(
                    &memory.id,
                    1,
                    "Revised",
                    "Updated",
                    revised.clone(),
                    &revision_anchors,
                    "agent revision",
                    "old-revise",
                )
                .unwrap();
            let reopened = ArtMcpServer::open(&paths, vault.agent_id().clone(), [4; 32]).unwrap();
            value["memory_id"] = json!(memory.id);
            value["expected_revision"] = json!(1);
            value["title"] = json!("Revised");
            value["summary"] = json!("Updated");
            value["payload"] = serde_json::to_value(revised).unwrap();
            value["idempotency_key"] = json!("old-revise");
            let replay = call(&reopened, value.clone()).await.unwrap();
            assert_eq!(replay["revision"], 2);
            assert_eq!(replay["origin"], "legacy_unspecified");
            // The historical revision adapter ignored scope fields entirely.
            value["scope_key"] = json!("");
            let replay = call(&reopened, value.clone()).await.unwrap();
            assert_eq!(replay["revision"], 2);
            assert_eq!(replay["origin"], "legacy_unspecified");
            value["title"] = json!("conflicting retry");
            assert!(
                call(&reopened, value)
                    .await
                    .unwrap_err()
                    .contains("ART_DUPLICATE_CONFLICT")
            );
        }
    }
    drop(server);
}
