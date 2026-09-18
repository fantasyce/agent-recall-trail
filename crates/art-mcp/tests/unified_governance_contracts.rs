use art_agent_store::{
    AgentVault, AutoMemoryConfigStore, IntakeAttribution, IntakeOrigin, MemoryIntakeRequest,
};
use art_domain::{
    agent::AgentId,
    anchor::{AnchorKind, SourceAnchor},
    memory::{MemoryArtifact, MemoryPayload, MemoryScope, SemanticPayload, Sensitivity},
};
use art_knowledge::KnowledgeVault;
use art_mcp::governance_ui::{GovernanceUiManager, UiView};
use chrono::Utc;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tempfile::tempdir;

fn input(agent: &AgentId, key: &str) -> MemoryIntakeRequest {
    MemoryIntakeRequest {
        capture_origin: Some(IntakeOrigin::AgentInitiated),
        memory: MemoryArtifact::new(
            agent.clone(),
            key,
            "Bounded correction",
            MemoryPayload::Semantic(SemanticPayload {
                statement: format!("Use the verified procedure for {key}."),
                applicability: "This repository".into(),
                exceptions: vec![],
            }),
            MemoryScope::Repository("art".into()),
            Sensitivity::Internal,
            Utc::now(),
        )
        .unwrap(),
        anchors: vec![
            SourceAnchor::new(
                agent.clone(),
                AnchorKind::UserStatement,
                format!("user:{key}"),
                None,
                json!({"evidence_date":"2026-09-17"}),
                Sensitivity::Internal,
                Utc::now(),
            )
            .unwrap(),
        ],
        idempotency_key: key.into(),
        attribution: IntakeAttribution::agent_asserted(None, None),
        request_basis: None,
        value_reason: Some("Avoid repeating a resolved investigation".into()),
        target_memory_id: None,
        expected_revision: None,
        hook_trigger_receipt_id: None,
    }
}

async fn get(
    client: &Client,
    session: &art_mcp::governance_ui::GovernanceUiSession,
    path: &str,
) -> Value {
    client
        .get(session.url.join(path).unwrap())
        .query(&[("session", &session.capability)])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn ordinary_intake_is_visible_with_origin_shared_budget_and_review_metadata() {
    let root = tempdir().unwrap();
    let agent: AgentId = "dsh-primary".parse().unwrap();
    let vault = AgentVault::open(root.path().join("private.sqlite3"), agent.clone()).unwrap();
    let config = AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:test").unwrap();
    vault.intake(&config, input(&agent, "ordinary")).unwrap();
    let manager = GovernanceUiManager::new(
        KnowledgeVault::open(root.path().join("knowledge"), [71; 32]).unwrap(),
        agent,
        "a".repeat(64),
    )
    .with_auto_memory(config, vault.clone());
    let client = Client::new();
    let settings = manager.open_session(UiView::Settings, None).await.unwrap();
    let status = get(&client, &settings, "/api/auto-memory").await;
    assert_eq!(status["host_support"]["dsh"]["agent_initiated"], true);
    assert_eq!(status["host_support"]["dsh"]["hook_triggered"], false);
    assert_eq!(
        status["automatic_origins"],
        json!(["agent_initiated", "hook_triggered"])
    );
    assert_eq!(status["diagnostics"]["pending_count"], 1);
    assert_eq!(
        status["diagnostics"]["origins"]["agent_initiated"]["last_disposition"],
        "pending_review"
    );
    assert_eq!(status["diagnostics"]["shared_budget"][0]["used"], 1);
    assert_eq!(status["diagnostics"]["shared_budget"][0]["degraded"], true);
    assert_eq!(status["diagnostics"]["intake_summary"]["pending_review"], 1);
    assert_eq!(status["diagnostics"]["intake_summary"]["activated"], 0);
    assert_eq!(status["diagnostics"]["trigger_summary"]["accepted"], 0);
    let pending = manager.open_session(UiView::Pending, None).await.unwrap();
    let list = get(&client, &pending, "/api/memory-candidates").await;
    assert_eq!(list["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(list["candidates"][0]["origin"], "agent_initiated");
    assert_eq!(
        list["candidates"][0]["value_reason"],
        "Avoid repeating a resolved investigation"
    );
    assert_eq!(
        list["candidates"][0]["anchors"][0]["metadata"]["evidence_date"],
        "2026-09-17"
    );
    let bootstrap = get(&client, &pending, "/api/bootstrap").await;
    let candidate = &list["candidates"][0]["artifact"];
    let response = client.post(pending.url.join("/api/memory-candidate-review").unwrap()).header("origin",&pending.origin).header("x-art-csrf",bootstrap["csrf_token"].as_str().unwrap())
        .json(&json!({"session":pending.capability,"memory_id":candidate["id"],"revision":1,"action":"edit_confirm","reason":"Reviewed correction","title":"Reviewed ordinary","summary":"Reviewed","payload":candidate["payload"]})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        vault
            .read(candidate["id"].as_str().unwrap())
            .unwrap()
            .current_revision,
        2
    );
}

#[tokio::test]
async fn revision_review_preserves_active_until_authenticated_exact_revision_decision() {
    for action in ["confirm", "edit_confirm", "reject", "stale"] {
        let root = tempdir().unwrap();
        let agent: AgentId = "codex-primary".parse().unwrap();
        let vault = AgentVault::open(root.path().join("private.sqlite3"), agent.clone()).unwrap();
        let config = AutoMemoryConfigStore::new(root.path());
        config.set_enabled(true, "human:test").unwrap();
        let original = input(&agent, "original");
        let id = original.memory.id.clone();
        vault
            .capture(&original.memory, &original.anchors, "initial")
            .unwrap();
        vault
            .assure(
                &id,
                1,
                art_domain::anchor::AssuranceOutcome::Corroborated,
                "human:test",
                "Reviewed",
            )
            .unwrap();
        let mut revision = input(&agent, "replacement");
        revision.target_memory_id = Some(id.clone());
        revision.expected_revision = Some(1);
        let result = vault.intake(&config, revision).unwrap();
        let manager = GovernanceUiManager::new(
            KnowledgeVault::open(root.path().join("knowledge"), [72; 32]).unwrap(),
            agent,
            "b".repeat(64),
        )
        .with_auto_memory(config.clone(), vault.clone());
        let client = Client::new();
        let pending = manager.open_session(UiView::Pending, None).await.unwrap();
        let bootstrap = get(&client, &pending, "/api/bootstrap").await;
        let list = get(&client, &pending, "/api/memory-candidates").await;
        assert_eq!(list["revision_proposals"].as_array().unwrap().len(), 1);
        assert_eq!(
            list["revision_proposals"][0]["current_artifact"]["title"],
            "original"
        );
        assert_eq!(
            list["revision_proposals"][0]["artifact"]["title"],
            "replacement"
        );
        assert_eq!(list["revision_proposals"][0]["expected_revision"], 1);
        config.set_enabled(false, "human:test").unwrap();
        let mut body = json!({"session":pending.capability,"proposal_id":result.proposal_id,"expected_revision":1,"action":if action=="stale" {"confirm"} else {action},"reason":"Human inspected original and proposed source"});
        if action == "edit_confirm" {
            body["title"] = json!("Edited replacement");
            body["summary"] = json!("Reviewed correction");
            body["payload"] = json!(original.memory.payload);
        }
        for (origin, csrf) in [
            (
                "https://hostile.invalid",
                bootstrap["csrf_token"].as_str().unwrap(),
            ),
            (pending.origin.as_str(), "wrong-csrf"),
        ] {
            let denied = client
                .post(pending.url.join("/api/memory-revision-review").unwrap())
                .header("origin", origin)
                .header("x-art-csrf", csrf)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(denied.status(), StatusCode::FORBIDDEN);
            assert_eq!(vault.read(&id).unwrap().current_revision, 1);
        }
        for (field, value, expected) in [
            (
                "session",
                json!("unknown-session"),
                StatusCode::UNAUTHORIZED,
            ),
            ("expected_revision", json!(2), StatusCode::CONFLICT),
            (
                "actor",
                json!("human:forged"),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ] {
            let mut invalid = body.clone();
            invalid[field] = value;
            let denied = client
                .post(pending.url.join("/api/memory-revision-review").unwrap())
                .header("origin", &pending.origin)
                .header("x-art-csrf", bootstrap["csrf_token"].as_str().unwrap())
                .json(&invalid)
                .send()
                .await
                .unwrap();
            assert_eq!(denied.status(), expected, "field: {field}");
            assert_eq!(vault.read(&id).unwrap().current_revision, 1);
        }
        if action == "edit_confirm" {
            let mut sensitive = body.clone();
            sensitive["payload"]["data"]["statement"] =
                json!("password = test-only-revision-marker");
            let denied = client
                .post(pending.url.join("/api/memory-revision-review").unwrap())
                .header("origin", &pending.origin)
                .header("x-art-csrf", bootstrap["csrf_token"].as_str().unwrap())
                .json(&sensitive)
                .send()
                .await
                .unwrap();
            assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
            assert_eq!(vault.read(&id).unwrap().current_revision, 1);
            assert_eq!(vault.pending_revision_proposals().unwrap().len(), 1);
        }
        if action == "stale" {
            let fresh = input(vault.agent_id(), "fresh-baseline");
            vault
                .revise(
                    &id,
                    1,
                    "New active baseline",
                    "Current",
                    fresh.memory.payload,
                    &fresh.anchors,
                    "New evidence",
                    "new-baseline",
                )
                .unwrap();
        }
        let response = client
            .post(pending.url.join("/api/memory-revision-review").unwrap())
            .header("origin", &pending.origin)
            .header("x-art-csrf", bootstrap["csrf_token"].as_str().unwrap())
            .json(&body)
            .send()
            .await
            .unwrap();
        if action == "stale" {
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert_eq!(vault.read(&id).unwrap().title, "New active baseline");
            assert_eq!(vault.pending_revision_proposals().unwrap().len(), 1);
            let before = serde_json::to_value(vault.read(&id).unwrap()).unwrap();
            body["action"] = json!("reject");
            let rejected = client
                .post(pending.url.join("/api/memory-revision-review").unwrap())
                .header("origin", &pending.origin)
                .header("x-art-csrf", bootstrap["csrf_token"].as_str().unwrap())
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(rejected.status(), StatusCode::OK);
            let receipt: Value = rejected.json().await.unwrap();
            assert_eq!(receipt["action"], "reject");
            assert_eq!(receipt["expected_revision"], 1);
            assert_eq!(receipt["revision"], 2);
            assert_eq!(receipt["actor"], "human:local-governance-ui");
            assert!(vault.pending_revision_proposals().unwrap().is_empty());
            assert_eq!(
                serde_json::to_value(vault.read(&id).unwrap()).unwrap(),
                before
            );
            assert_eq!(vault.memory_revision_reviews().unwrap()[0], receipt);
            continue;
        }
        assert_eq!(response.status(), StatusCode::OK);
        let receipt: Value = response.json().await.unwrap();
        assert_eq!(receipt["actor"], "human:local-governance-ui");
        let current = vault.read(&id).unwrap();
        assert_eq!(
            current.current_revision,
            if action == "reject" { 1 } else { 2 }
        );
        assert_eq!(current.status, art_domain::memory::MemoryStatus::Active);
        if action == "edit_confirm" {
            assert_eq!(current.title, "Edited replacement");
        }
        assert!(vault.pending_revision_proposals().unwrap().is_empty());
        let audit = manager.open_session(UiView::Audit, None).await.unwrap();
        let audit_body = get(&client, &audit, "/api/bootstrap").await;
        assert_eq!(audit_body["memory_reviews"][0]["action"], action);
        assert_eq!(
            audit_body["memory_reviews"][0]["actor"],
            "human:local-governance-ui"
        );
        let stale = client
            .post(pending.url.join("/api/memory-revision-review").unwrap())
            .header("origin", &pending.origin)
            .header("x-art-csrf", bootstrap["csrf_token"].as_str().unwrap())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
    }
}
