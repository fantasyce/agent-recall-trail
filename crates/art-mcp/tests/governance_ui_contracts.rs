use std::str::FromStr;

use art_domain::{
    agent::AgentId,
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor},
};
use art_knowledge::{DelegationMode, GovernanceSnapshot, KnowledgeVault};
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
    let remaining = session.expires_at - chrono::Utc::now();
    assert!(remaining >= chrono::Duration::minutes(29));
    assert!(remaining <= chrono::Duration::minutes(30));

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

#[tokio::test]
async fn exact_review_and_publish_return_authoritative_receipts() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [65_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("governance.publish", "Publish", "review then publish"),
            vec![source(&agent, "artm_publish")],
            "publish-receipt",
        )
        .unwrap();
    let submitted = GovernanceSnapshot::from_proposal(&proposal);
    let manager = GovernanceUiManager::new(vault.clone(), agent, "e".repeat(64));
    let session = manager
        .open_session(
            UiView::Pending,
            Some((proposal.id.clone(), proposal.revision)),
        )
        .await
        .unwrap();
    let client = Client::new();
    let bootstrap: serde_json::Value = client
        .get(session.url.join("/api/bootstrap").unwrap())
        .query(&[("session", session.capability.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let csrf = bootstrap["csrf_token"].as_str().unwrap();
    let reviewed = client
        .post(session.url.join("/api/review").unwrap())
        .header("origin", &session.origin)
        .header("x-art-csrf", csrf)
        .json(&serde_json::json!({
            "session": session.capability,
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": "submitted",
            "draft_hash": submitted.draft_hash,
            "source_set_hash": submitted.source_set_hash,
            "decision": "approved",
            "reason": "  source and scope checked  "
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reviewed.status(), StatusCode::OK);
    let reviewed: serde_json::Value = reviewed.json().await.unwrap();
    assert_eq!(reviewed["schema"], "art.governance.review-receipt.v1");
    assert_eq!(reviewed["status"], "approved");
    assert_eq!(reviewed["review"]["decision"], "approved");
    assert_eq!(reviewed["review"]["reason"], "source and scope checked");

    let approved = GovernanceSnapshot::from_proposal(&vault.proposal(&proposal.id).unwrap());
    let published = client
        .post(session.url.join("/api/publish").unwrap())
        .header("origin", &session.origin)
        .header("x-art-csrf", csrf)
        .json(&serde_json::json!({
            "session": session.capability,
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": "approved",
            "draft_hash": approved.draft_hash,
            "source_set_hash": approved.source_set_hash,
            "predicted_edition_number": 1,
            "confirm": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), StatusCode::OK);
    let published: serde_json::Value = published.json().await.unwrap();
    assert_eq!(published["schema"], "art.governance.publication-receipt.v1");
    assert_eq!(published["edition_number"], 1);
    assert!(published["edition_id"].as_str().unwrap().starts_with("arke_"));
    assert_eq!(published["markdown_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(published["manifest_sha256"].as_str().unwrap().len(), 64);
    assert!(published["published_at"].as_str().unwrap().contains('T'));
    assert_eq!(
        vault.proposal(&proposal.id).unwrap().status,
        art_domain::knowledge::ProposalStatus::Materialized
    );
}

#[tokio::test]
async fn stale_or_invalid_review_requests_fail_without_a_governance_write() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [66_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("governance.reject", "Reject", "unsafe basis"),
            vec![source(&agent, "artm_reject")],
            "reject-receipt",
        )
        .unwrap();
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    let manager = GovernanceUiManager::new(vault.clone(), agent, "f".repeat(64));
    let session = manager
        .open_session(
            UiView::Pending,
            Some((proposal.id.clone(), proposal.revision)),
        )
        .await
        .unwrap();
    let client = Client::new();
    let bootstrap: serde_json::Value = client
        .get(session.url.join("/api/bootstrap").unwrap())
        .query(&[("session", session.capability.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let csrf = bootstrap["csrf_token"].as_str().unwrap();

    for (draft_hash, reason, expected) in [
        ("0".repeat(64), "valid reason".to_owned(), StatusCode::CONFLICT),
        (snapshot.draft_hash.clone(), " ".to_owned(), StatusCode::BAD_REQUEST),
        (
            snapshot.draft_hash.clone(),
            "x".repeat(1_001),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = client
            .post(session.url.join("/api/review").unwrap())
            .header("origin", &session.origin)
            .header("x-art-csrf", csrf)
            .json(&serde_json::json!({
                "session": session.capability,
                "proposal_id": proposal.id,
                "revision": proposal.revision,
                "status": "submitted",
                "draft_hash": draft_hash,
                "source_set_hash": snapshot.source_set_hash,
                "decision": "rejected",
                "reason": reason
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(vault
            .proposal_reviews(&proposal.id, proposal.revision)
            .unwrap()
            .is_empty());
    }

    let rejected = client
        .post(session.url.join("/api/review").unwrap())
        .header("origin", &session.origin)
        .header("x-art-csrf", csrf)
        .json(&serde_json::json!({
            "session": session.capability,
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": "submitted",
            "draft_hash": snapshot.draft_hash,
            "source_set_hash": snapshot.source_set_hash,
            "decision": "rejected",
            "reason": "unsafe and unsupported"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::OK);
    let rejected: serde_json::Value = rejected.json().await.unwrap();
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(vault.proposal_reviews(&proposal.id, 1).unwrap().len(), 1);

    let replay = client
        .post(session.url.join("/api/review").unwrap())
        .header("origin", &session.origin)
        .header("x-art-csrf", csrf)
        .json(&serde_json::json!({
            "session": session.capability,
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": "submitted",
            "draft_hash": snapshot.draft_hash,
            "source_set_hash": snapshot.source_set_hash,
            "decision": "approved",
            "reason": "replay"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    assert_eq!(vault.proposal_reviews(&proposal.id, 1).unwrap().len(), 1);
}

#[tokio::test]
async fn governance_ui_serves_the_accessible_detail_workspace_assets() {
    let root = tempdir().unwrap();
    let manager = GovernanceUiManager::new(
        KnowledgeVault::open(root.path(), [67_u8; 32]).unwrap(),
        AgentId::from_str("codex-primary").unwrap(),
        "1".repeat(64),
    );
    let session = manager.open_session(UiView::Pending, None).await.unwrap();
    let client = Client::new();
    let html = client
        .get(session.url.clone())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("<dialog id=\"proposal-dialog\""));
    assert!(html.contains("aria-labelledby=\"proposal-title\""));
    assert!(html.contains("aria-describedby=\"proposal-description\""));
    assert!(html.contains("role=\"tablist\""));
    assert!(html.contains("id=\"review-reason\""));
    assert!(html.contains("maxlength=\"1000\""));
    assert!(html.contains("aria-live=\"polite\""));

    let script = client
        .get(session.url.join("/app.js").unwrap())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(script.contains("openProposal"));
    assert!(script.contains("loadProposalDetail"));
    assert!(script.contains("enterConflict"));
    assert!(script.contains("enterExpired"));
    assert!(script.contains("审核详情"));
    assert!(!script.contains("window.prompt"));
    assert!(!script.contains("window.confirm"));

    let styles = client
        .get(session.url.join("/styles.css").unwrap())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(styles.contains("width: min(880px, 72vw)"));
    assert!(styles.contains("@media (max-width: 759px)"));
    assert!(styles.contains("@media (prefers-reduced-motion: reduce)"));
}

#[tokio::test]
async fn governance_session_view_limits_each_mutation_surface() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let host = "2".repeat(64);
    let vault = KnowledgeVault::open(root.path(), [68_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal("governance.view", "View bound", "bounded"),
            vec![source(&agent, "artm_view")],
            "view-bound",
        )
        .unwrap();
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    let manager = GovernanceUiManager::new(vault.clone(), agent.clone(), host.clone());
    let settings = manager.open_session(UiView::Settings, None).await.unwrap();
    let pending = manager.open_session(UiView::Pending, None).await.unwrap();
    let client = Client::new();
    let settings_bootstrap: serde_json::Value = client
        .get(settings.url.join("/api/bootstrap").unwrap())
        .query(&[("session", settings.capability.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pending_bootstrap: serde_json::Value = client
        .get(pending.url.join("/api/bootstrap").unwrap())
        .query(&[("session", pending.capability.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let review = client
        .post(settings.url.join("/api/review").unwrap())
        .header("origin", &settings.origin)
        .header("x-art-csrf", settings_bootstrap["csrf_token"].as_str().unwrap())
        .json(&serde_json::json!({
            "session": settings.capability,
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": "submitted",
            "draft_hash": snapshot.draft_hash,
            "source_set_hash": snapshot.source_set_hash,
            "decision": "approved",
            "reason": "wrong view"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(review.status(), StatusCode::FORBIDDEN);
    assert!(vault.proposal_reviews(&proposal.id, 1).unwrap().is_empty());

    let delegation = client
        .post(pending.url.join("/api/delegation").unwrap())
        .header("origin", &pending.origin)
        .header("x-art-csrf", pending_bootstrap["csrf_token"].as_str().unwrap())
        .json(&serde_json::json!({
            "session": pending.capability,
            "mode": "delegated_local"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(delegation.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        vault.delegation_mode(&agent, &host).unwrap(),
        DelegationMode::HumanReview
    );
}
