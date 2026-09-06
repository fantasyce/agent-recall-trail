use std::str::FromStr;

use art_domain::agent::AgentId;
use art_knowledge::{DelegationMode, KnowledgeVault};
use art_mcp::governance_ui::{GovernanceUiManager, UiView};
use reqwest::{Client, StatusCode};
use tempfile::tempdir;

#[tokio::test]
async fn governance_ui_is_loopback_session_bound_and_changes_real_policy() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let host_hash = "a".repeat(64);
    let vault = KnowledgeVault::open(root.path(), [61_u8; 32]).unwrap();
    let manager = GovernanceUiManager::new(vault.clone(), agent.clone(), host_hash.clone());
    let session = manager.open_session(UiView::Settings, None).await.unwrap();

    assert_eq!(session.url.host_str(), Some("127.0.0.1"));
    assert_ne!(session.url.port(), Some(80));
    assert!(session.expires_at > chrono::Utc::now());

    let client = Client::new();
    let page = client.get(session.url.clone()).send().await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(page.text().await.unwrap().contains("ART governance"));

    let bootstrap_url = session.url.join("/api/bootstrap").unwrap();
    let bootstrap = client
        .get(bootstrap_url.clone())
        .query(&[("session", session.capability.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload: serde_json::Value = bootstrap.json().await.unwrap();
    assert_eq!(payload["governance_mode"], "human_review");
    assert_eq!(payload["bound_agent_id"], "codex-primary");

    let rejected = client
        .post(session.url.join("/api/delegation").unwrap())
        .header("origin", "https://hostile.example")
        .header("x-art-csrf", payload["csrf_token"].as_str().unwrap())
        .json(&serde_json::json!({
            "session": session.capability,
            "mode": "delegated_local"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let accepted = client
        .post(session.url.join("/api/delegation").unwrap())
        .header("origin", session.origin.as_str())
        .header("x-art-csrf", payload["csrf_token"].as_str().unwrap())
        .json(&serde_json::json!({
            "session": session.capability,
            "mode": "delegated_local"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(
        vault.delegation_mode(&agent, &host_hash).unwrap(),
        DelegationMode::DelegatedLocal
    );

    let missing = client
        .get(bootstrap_url)
        .query(&[("session", "invalid")])
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn governance_ui_session_is_revision_scoped_and_reuses_one_server() {
    let root = tempdir().unwrap();
    let manager = GovernanceUiManager::new(
        KnowledgeVault::open(root.path(), [62_u8; 32]).unwrap(),
        AgentId::from_str("codex-primary").unwrap(),
        "b".repeat(64),
    );
    let first = manager
        .open_session(UiView::Pending, Some(("artp_fixture".into(), 3)))
        .await
        .unwrap();
    let second = manager.open_session(UiView::Audit, None).await.unwrap();

    assert_eq!(first.url.port(), second.url.port());
    assert_ne!(first.capability, second.capability);
    assert_eq!(first.proposal_id.as_deref(), Some("artp_fixture"));
    assert_eq!(first.revision, Some(3));
}
