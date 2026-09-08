use std::str::FromStr;

use art_domain::{
    agent::AgentId,
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor},
};
use art_knowledge::{DelegationMode, KnowledgeVault};
use art_mcp::governance_ui::{GovernanceUiManager, UiView};
use reqwest::{Client, StatusCode};
use tempfile::tempdir;

fn source(agent: &AgentId, source_id: &str) -> ProposalSourceLock {
    ProposalSourceLock {
        source_type: ProposalSourceType::PrivateMemory,
        owner_agent_id: Some(agent.clone()),
        source_id: source_id.into(),
        source_revision: Some(3),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: Some("b".repeat(64)),
        approved_excerpt_hash: Some("c".repeat(64)),
        use_grant_id: None,
    }
}

fn assert_security_headers(response: &reqwest::Response) {
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

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

#[tokio::test]
async fn proposal_detail_is_summary_separated_exact_authorized_and_sanitized() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [63_u8; 32]).unwrap();
    let malicious = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal(
                "governance.safe-render",
                "Safe rendering",
                "# Heading\n\n<script>steal()</script>\n<a href=\"javascript:steal()\" onclick=\"steal()\">bad</a>\n<img src=\"https://tracker.example/pixel\" onerror=\"steal()\">\n<iframe src=\"https://tracker.example\"></iframe>\n<style>body{display:none}</style>\n\n| A | B |\n| - | - |\n| one | two |",
            ),
            vec![source(&agent, "artm_secret_body_must_not_appear")],
            "safe-render",
        )
        .unwrap();
    let second = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("governance.second", "Second", "second private draft"),
            vec![source(&agent, "artm_second")],
            "second",
        )
        .unwrap();
    let manager = GovernanceUiManager::new(vault, agent, "c".repeat(64));
    let exact = manager
        .open_session(
            UiView::Pending,
            Some((malicious.id.clone(), malicious.revision)),
        )
        .await
        .unwrap();
    let client = Client::new();

    let page = client.get(exact.url.clone()).send().await.unwrap();
    assert_security_headers(&page);
    let bootstrap = client
        .get(exact.url.join("/api/bootstrap").unwrap())
        .query(&[("session", exact.capability.as_str())])
        .send()
        .await
        .unwrap();
    assert_security_headers(&bootstrap);
    let bootstrap: serde_json::Value = bootstrap.json().await.unwrap();
    assert_eq!(bootstrap["actionable_count"], 1);
    assert_eq!(bootstrap["proposals"].as_array().unwrap().len(), 1);
    let bootstrap_text = serde_json::to_string(&bootstrap).unwrap();
    assert!(!bootstrap_text.contains("<script>"));
    assert!(!bootstrap_text.contains("second private draft"));
    assert!(!bootstrap_text.contains("artm_secret_body_must_not_appear"));

    let detail = client
        .get(exact.url.join("/api/proposal-detail").unwrap())
        .query(&[
            ("session", exact.capability.as_str()),
            ("proposal_id", malicious.id.as_str()),
            ("revision", "1"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    assert_security_headers(&detail);
    let detail: serde_json::Value = detail.json().await.unwrap();
    assert_eq!(detail["schema"], "art.governance.proposal-detail.v1");
    assert_eq!(detail["proposal"]["proposal_id"], malicious.id);
    assert_eq!(detail["proposal"]["revision"], 1);
    assert_eq!(detail["comparison"]["kind"], "first_edition");
    assert_eq!(detail["predicted_edition_number"], 1);
    assert_eq!(detail["sources"].as_array().unwrap().len(), 1);
    let rendered = detail["content"]["rendered_html"].as_str().unwrap();
    assert!(rendered.contains("<table>"));
    for forbidden in [
        "<script",
        "javascript:",
        "onclick",
        "onerror",
        "<iframe",
        "<style",
        "https://tracker.example",
    ] {
        assert!(!rendered.contains(forbidden), "rendered HTML kept {forbidden}");
    }
    assert!(!rendered.contains(&exact.capability));

    let denied = client
        .get(exact.url.join("/api/proposal-detail").unwrap())
        .query(&[
            ("session", exact.capability.as_str()),
            ("proposal_id", second.id.as_str()),
            ("revision", "1"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn proposal_detail_compares_against_hash_verified_current_edition() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [64_u8; 32]).unwrap();
    let first = vault
        .propose(
            &agent,
            KnowledgeDraft {
                knowledge_key: "governance.diff".into(),
                title: "First".into(),
                applicability: "review workspaces".into(),
                markdown: "keep\nremove".into(),
                sensitivity: art_domain::memory::Sensitivity::Internal,
                risk: art_domain::knowledge::RiskLevel::Normal,
            },
            vec![source(&agent, "artm_first")],
            "diff-first",
        )
        .unwrap();
    vault
        .approve(
            &first.id,
            first.revision,
            ReviewActor::Human("reviewer".into()),
            "reviewed",
        )
        .unwrap();
    let edition = vault.publish(&first.id, first.revision, true).unwrap();
    let replacement = vault
        .propose(
            &agent,
            KnowledgeDraft {
                knowledge_key: "governance.diff".into(),
                title: "Replacement".into(),
                applicability: "review workspaces".into(),
                markdown: "keep\nadd".into(),
                sensitivity: art_domain::memory::Sensitivity::Internal,
                risk: art_domain::knowledge::RiskLevel::Normal,
            },
            vec![source(&agent, "artm_replacement")],
            "diff-replacement",
        )
        .unwrap();
    let manager = GovernanceUiManager::new(vault, agent, "d".repeat(64));
    let session = manager.open_session(UiView::Pending, None).await.unwrap();
    let response = Client::new()
        .get(session.url.join("/api/proposal-detail").unwrap())
        .query(&[
            ("session", session.capability.as_str()),
            ("proposal_id", replacement.id.as_str()),
            ("revision", "1"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: serde_json::Value = response.json().await.unwrap();
    assert_eq!(detail["current_edition"]["edition_id"], edition.edition_id);
    assert_eq!(detail["comparison"]["kind"], "line_diff");
    let comparison = serde_json::to_string(&detail["comparison"]).unwrap();
    assert!(comparison.contains("removed"));
    assert!(comparison.contains("added"));
    assert!(comparison.contains("remove"));
    assert!(comparison.contains("add"));
}
