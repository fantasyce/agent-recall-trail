use std::{
    collections::VecDeque,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use art_domain::agent::{AgentId, ArtPaths};
use art_mcp::ArtMcpServer;
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientInfo, ElicitRequestParams, ElicitResult,
        ElicitationAction, ElicitationCapability, FormElicitationCapability, Implementation,
    },
    service::RequestContext,
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[derive(Clone)]
struct ElicitationClient {
    responses: Arc<Mutex<VecDeque<ElicitResult>>>,
    requests: Arc<Mutex<Vec<ElicitRequestParams>>>,
    supports_elicitation: bool,
    empty_elicitation_capability: bool,
    stale_on_request: Option<usize>,
    hang_on_request: Option<usize>,
    private_db: PathBuf,
}

#[allow(clippy::unused_async_trait_impl)]
impl ClientHandler for ElicitationClient {
    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, rmcp::ErrorData> {
        let request_count = {
            let mut requests = self.requests.lock().unwrap();
            requests.push(request);
            requests.len()
        };
        if self.stale_on_request == Some(request_count) {
            let connection = rusqlite::Connection::open(&self.private_db).unwrap();
            let anchor_id: String = connection
                .query_row(
                    "SELECT id FROM source_anchors ORDER BY id LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            connection.execute(
                "INSERT INTO source_events(id,anchor_id,event_type,new_digest,actor,reason,created_at) VALUES (?1,?2,'revoked',NULL,'test-human','source invalidated during elicitation',?3)",
                rusqlite::params![format!("artse_test_{request_count}"), anchor_id, chrono::Utc::now().to_rfc3339()],
            ).unwrap();
        }
        if self.hang_on_request == Some(request_count) {
            return std::future::pending().await;
        }
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("queued response"))
    }

    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        info.capabilities = if self.supports_elicitation {
            ClientCapabilities::builder()
                .enable_elicitation_with(
                    ElicitationCapability::new()
                        .with_form(FormElicitationCapability::new().with_schema_validation(true)),
                )
                .build()
        } else if self.empty_elicitation_capability {
            ClientCapabilities::builder().enable_elicitation().build()
        } else {
            ClientCapabilities::default()
        };
        info.client_info = Implementation::new("art-test-reviewer", "1");
        info
    }
}

struct Harness {
    root: TempDir,
    client: rmcp::service::RunningService<RoleClient, ElicitationClient>,
    requests: Arc<Mutex<Vec<ElicitRequestParams>>>,
    server_task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Harness {
    async fn start(supports_elicitation: bool, responses: Vec<ElicitResult>) -> Self {
        Self::start_with_options(supports_elicitation, false, responses, None, None).await
    }

    async fn start_empty_elicitation(responses: Vec<ElicitResult>) -> Self {
        Self::start_with_options(false, true, responses, None, None).await
    }

    async fn start_with_stale_request(
        supports_elicitation: bool,
        responses: Vec<ElicitResult>,
        stale_on_request: Option<usize>,
    ) -> Self {
        Self::start_with_options(
            supports_elicitation,
            false,
            responses,
            stale_on_request,
            None,
        )
        .await
    }

    async fn start_with_timeout(responses: Vec<ElicitResult>) -> Self {
        Self::start_with_options(true, false, responses, None, Some(1)).await
    }

    async fn start_with_options(
        supports_elicitation: bool,
        empty_elicitation_capability: bool,
        responses: Vec<ElicitResult>,
        stale_on_request: Option<usize>,
        hang_on_request: Option<usize>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
        let mut server = ArtMcpServer::open(
            &paths,
            AgentId::from_str("codex-primary").unwrap(),
            [51; 32],
        )
        .unwrap();
        if hang_on_request.is_some() {
            server.test_only_set_elicitation_timeout(std::time::Duration::from_millis(25));
        }
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = ElicitationClient {
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::clone(&requests),
            supports_elicitation,
            empty_elicitation_capability,
            stale_on_request,
            hang_on_request,
            private_db: root
                .path()
                .join("data/art/agents/codex-primary/art.sqlite3"),
        };
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            let running = server.serve(server_transport).await?;
            running.waiting().await?;
            Ok(())
        });
        let client = handler.serve(client_transport).await.unwrap();
        Self {
            root,
            client,
            requests,
            server_task,
        }
    }

    async fn start_delegated() -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
        let mut server = ArtMcpServer::open(
            &paths,
            AgentId::from_str("codex-primary").unwrap(),
            [51; 32],
        )
        .unwrap();
        server.test_only_set_delegation_mode(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = ElicitationClient {
            responses: Arc::new(Mutex::new(VecDeque::new())),
            requests: Arc::clone(&requests),
            supports_elicitation: false,
            empty_elicitation_capability: false,
            stale_on_request: None,
            hang_on_request: None,
            private_db: root
                .path()
                .join("data/art/agents/codex-primary/art.sqlite3"),
        };
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            let running = server.serve(server_transport).await?;
            running.waiting().await?;
            Ok(())
        });
        let client = handler.serve(client_transport).await.unwrap();
        Self {
            root,
            client,
            requests,
            server_task,
        }
    }

    async fn call(&self, name: &str, arguments: Value) -> Value {
        let arguments = arguments.as_object().unwrap().clone();
        let result = self
            .client
            .peer()
            .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
            .await
            .unwrap();
        result.structured_content.unwrap_or_else(|| {
            let text = result
                .content
                .first()
                .and_then(|block| block.as_text())
                .expect("text or structured tool output");
            serde_json::from_str(&text.text).expect("JSON tool output")
        })
    }

    async fn proposal(&self) -> (String, u32) {
        let memory = self.call("art_memory_capture", json!({
            "title":"Elicitation fixture",
            "summary":"A bounded source for protocol tests.",
            "payload": {"kind":"semantic","data":{
                "statement":"The elicitation fixture is valid.",
                "applicability":"ART protocol tests",
                "confidence":"high",
                "evidence_summary":"A local deterministic test fixture.",
                "revisit_when":null
            }},
            "scope_type":"repository",
            "scope_key":"agent-recall-trail",
            "sensitivity":"internal",
            "idempotency_key":"elicitation-memory",
            "anchors":[{"kind":"test_receipt","locator":"test://elicitation","source_version":"1","source_digest":null,"excerpt":"fixture","metadata":{}}],
            "unanchored_candidate":false,
            "no_persist_provenance":false
        })).await;
        let source = format!(
            "memory:{}@{}",
            memory["memory_id"].as_str().unwrap(),
            memory["revision"]
        );
        let proposal = self
            .call(
                "art_knowledge_propose",
                json!({
                    "knowledge_key":"elicitation-fixture",
                    "title":"Elicitation fixture",
                    "applicability":"ART protocol tests",
                    "markdown":"Complete proposal body for the reviewer.",
                    "sensitivity":"internal",
                    "source_refs":[source],
                    "idempotency_key":"elicitation-proposal"
                }),
            )
            .await;
        (
            proposal["proposal_id"].as_str().unwrap().to_owned(),
            u32::try_from(proposal["revision"].as_u64().unwrap()).unwrap(),
        )
    }

    async fn stop(self) {
        self.client.cancel().await.unwrap();
        self.server_task.await.unwrap().unwrap();
    }

    fn review_actor(&self, proposal_id: &str) -> String {
        let connection = rusqlite::Connection::open(
            self.root
                .path()
                .join("data/art/knowledge-vault/art-control.sqlite3"),
        )
        .unwrap();
        connection.query_row(
            "SELECT actor FROM proposal_reviews WHERE proposal_id=?1 ORDER BY decided_at DESC LIMIT 1",
            [proposal_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn set_proposal_risk(&self, proposal_id: &str, risk: &str) {
        let connection = rusqlite::Connection::open(
            self.root
                .path()
                .join("data/art/knowledge-vault/art-control.sqlite3"),
        )
        .unwrap();
        let draft: String = connection
            .query_row(
                "SELECT draft_json FROM knowledge_proposals WHERE id=?1",
                [proposal_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut draft: Value = serde_json::from_str(&draft).unwrap();
        draft["risk"] = json!(risk);
        connection
            .execute(
                "UPDATE knowledge_proposals SET draft_json=?2 WHERE id=?1",
                rusqlite::params![proposal_id, serde_json::to_string(&draft).unwrap()],
            )
            .unwrap();
    }

    fn edition_count(&self) -> u32 {
        let connection = rusqlite::Connection::open(
            self.root
                .path()
                .join("data/art/knowledge-vault/art-control.sqlite3"),
        )
        .unwrap();
        connection
            .query_row("SELECT COUNT(*) FROM edition_projections", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    async fn revise_proposal_source(&self, proposal_id: &str) {
        let connection = rusqlite::Connection::open(
            self.root
                .path()
                .join("data/art/knowledge-vault/art-control.sqlite3"),
        )
        .unwrap();
        let sources: String = connection
            .query_row(
                "SELECT sources_json FROM knowledge_proposals WHERE id=?1",
                [proposal_id],
                |row| row.get(0),
            )
            .unwrap();
        let sources: Value = serde_json::from_str(&sources).unwrap();
        let memory_id = sources[0]["source_id"].as_str().unwrap();
        let revision = sources[0]["source_revision"].as_u64().unwrap();
        let revised = self
            .call(
                "art_memory_capture",
                json!({
                    "memory_id":memory_id,
                    "expected_revision":revision,
                    "title":"Revised elicitation fixture",
                    "summary":"The source changed after proposal creation.",
                    "payload":{"kind":"semantic","data":{
                        "statement":"The elicitation fixture has changed.",
                        "applicability":"ART protocol tests",
                        "confidence":"high",
                        "evidence_summary":"A later deterministic source revision.",
                        "revisit_when":null
                    }},
                    "scope_type":"repository",
                    "scope_key":"agent-recall-trail",
                    "sensitivity":"internal",
                    "idempotency_key":"elicitation-memory-revision",
                    "anchors":[{"kind":"test_receipt","locator":"test://elicitation-revised","source_version":"2","source_digest":null,"excerpt":"revised fixture","metadata":{}}],
                    "unanchored_candidate":false,
                    "no_persist_provenance":false
                }),
            )
            .await;
        assert_eq!(revised["revision"], revision + 1);
    }
}

#[tokio::test]
async fn delegated_governance_publishes_without_elicitation_and_is_distinctly_labeled() {
    let harness = Harness::start_delegated().await;
    let (proposal_id, revision) = harness.proposal().await;

    let health = harness.call("art_health", json!({})).await;
    assert_eq!(health["governance_mode"], "delegated_local");
    let result = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"approve_and_publish",
                "proposal_id":proposal_id,
                "revision":revision
            }),
        )
        .await;

    assert_eq!(result["outcome"], "published", "{result}");
    assert_eq!(result["actor_type"], "agent_delegated");
    assert_eq!(result["proposal_status"], "materialized");
    assert!(result["edition_id"].as_str().unwrap().starts_with("arke_"));
    assert!(harness.requests.lock().unwrap().is_empty());
    assert_eq!(harness.edition_count(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn delegated_governance_fails_closed_when_persistent_policy_is_disabled() {
    let harness = Harness::start(false, vec![]).await;
    let (proposal_id, revision) = harness.proposal().await;

    let health = harness.call("art_health", json!({})).await;
    assert_eq!(health["governance_mode"], "human_review");
    let result = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"approve_and_publish",
                "proposal_id":proposal_id,
                "revision":revision
            }),
        )
        .await;

    assert_eq!(result["outcome"], "operator_action_required");
    assert_eq!(result["reason_code"], "DELEGATION_DISABLED");
    assert_eq!(harness.edition_count(), 0);
    assert!(harness.requests.lock().unwrap().is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn review_approval_is_collected_by_form_elicitation_and_committed() {
    let harness = Harness::start(
        true,
        vec![ElicitResult::new(ElicitationAction::Accept).with_content(
            json!({"decision":"approve","reason":"Reviewed against the complete draft."}),
        )],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;

    let result = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;

    assert_eq!(result["outcome"], "reviewed", "{result}");
    assert_eq!(result["proposal_status"], "approved");
    let actor = harness.review_actor(&proposal_id);
    assert!(actor.starts_with("mcp-elicitation:"));
    assert_ne!(actor, "Reviewed against the complete draft.");
    {
        let requests = harness.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            ..
        } = &requests[0]
        else {
            panic!("review must use form elicitation")
        };
        assert!(message.contains(&proposal_id));
        assert!(message.contains("Complete proposal body for the reviewer."));
        let schema = serde_json::to_value(requested_schema).unwrap();
        assert!(schema.to_string().contains("request_changes"));
        let allowed_root_fields = ["$schema", "type", "properties", "required"];
        assert!(
            schema
                .as_object()
                .unwrap()
                .keys()
                .all(|key| { allowed_root_fields.contains(&key.as_str()) }),
            "Codex rejects unsupported root Elicitation schema fields: {schema}"
        );
    }
    harness.stop().await;
}

#[tokio::test]
async fn review_request_changes_and_reject_are_human_form_decisions() {
    for (decision, expected) in [
        ("request_changes", "changes_requested"),
        ("reject", "rejected"),
    ] {
        let harness = Harness::start(
            true,
            vec![
                ElicitResult::new(ElicitationAction::Accept).with_content(json!({
                    "decision": decision,
                    "reason": format!("Human selected {decision} after reading the draft.")
                })),
            ],
        )
        .await;
        let (proposal_id, revision) = harness.proposal().await;
        let result = harness
            .call(
                "art_knowledge_governance",
                json!({
                    "operation":"review", "proposal_id":proposal_id, "revision":revision
                }),
            )
            .await;
        assert_eq!(result["outcome"], "reviewed");
        assert_eq!(result["proposal_status"], expected);
        assert!(
            harness
                .review_actor(&proposal_id)
                .starts_with("mcp-elicitation:")
        );
        harness.stop().await;
    }
}

#[tokio::test]
async fn unsupported_client_gets_cli_fallback_without_an_elicitation_request() {
    let harness = Harness::start(false, vec![]).await;
    let (proposal_id, revision) = harness.proposal().await;

    let result = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;

    assert_eq!(result["outcome"], "operator_action_required");
    assert_eq!(result["reason_code"], "ELICITATION_UNSUPPORTED");
    assert!(harness.requests.lock().unwrap().is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn empty_elicitation_capability_is_not_treated_as_form_support() {
    let harness = Harness::start_empty_elicitation(vec![]).await;
    let (proposal_id, revision) = harness.proposal().await;
    let result = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(result["reason_code"], "ELICITATION_UNSUPPORTED");
    assert!(harness.requests.lock().unwrap().is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn review_and_publish_require_two_distinct_elicitations() {
    let harness = Harness::start(true, vec![
        ElicitResult::new(ElicitationAction::Accept)
            .with_content(json!({"decision":"approve","reason":"Approved after reviewing the exact proposal."})),
        ElicitResult::new(ElicitationAction::Accept)
            .with_content(json!({"confirm":true})),
    ]).await;
    let (proposal_id, revision) = harness.proposal().await;

    let review = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(review["outcome"], "reviewed");
    let publish = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"publish", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;

    assert_eq!(publish["outcome"], "published", "{publish}");
    assert!(publish["edition_id"].as_str().unwrap().starts_with("arke_"));
    assert_eq!(publish["edition_number"], 1);
    {
        let requests = harness.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let messages: Vec<_> = requests
            .iter()
            .map(|request| match request {
                ElicitRequestParams::FormElicitationParams { message, .. } => message.as_str(),
                _ => panic!("governance must use form elicitation"),
            })
            .collect();
        assert!(messages[0].contains("Choose approve"));
        assert!(messages[1].contains("shared immutable Edition"));
    }
    harness.stop().await;
}

#[tokio::test]
async fn decline_and_false_confirmation_do_not_mutate_governance_state() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Decline),
            ElicitResult::new(ElicitationAction::Accept).with_content(
                json!({"decision":"approve","reason":"Approved on a later explicit review."}),
            ),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":false})),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;

    let declined_review = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(declined_review["outcome"], "declined");
    assert_eq!(declined_review["proposal_status"], "submitted");
    let approved = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(approved["proposal_status"], "approved");
    let unconfirmed = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"publish", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(unconfirmed["outcome"], "declined");
    assert_eq!(unconfirmed["reason_code"], "PUBLICATION_NOT_CONFIRMED");
    let published = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"publish", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(published["outcome"], "published");
    harness.stop().await;
}

#[tokio::test]
async fn malformed_review_content_fails_closed_and_can_be_retried() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"self_approve","reason":"Agent supplied"})),
            ElicitResult::new(ElicitationAction::Accept).with_content(
                json!({"decision":"reject","reason":"Human rejected the proposed wording."}),
            ),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;

    let malformed = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(malformed["outcome"], "operator_action_required");
    assert_eq!(malformed["reason_code"], "ELICITATION_INVALID_RESPONSE");
    assert_eq!(malformed["proposal_status"], "submitted");
    let rejected = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(rejected["outcome"], "reviewed");
    assert_eq!(rejected["proposal_status"], "rejected");
    harness.stop().await;
}

#[tokio::test]
async fn cancellation_blank_reason_and_oversized_reason_all_fail_closed() {
    let oversized = "x".repeat(1001);
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Cancel),
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":"   "})),
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":oversized})),
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":"Final valid human reason."})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;

    for (expected_outcome, expected_reason) in [
        ("cancelled", "USER_CANCELLED"),
        ("operator_action_required", "ELICITATION_INVALID_RESPONSE"),
        ("operator_action_required", "ELICITATION_INVALID_RESPONSE"),
    ] {
        let result = harness
            .call(
                "art_knowledge_governance",
                json!({
                    "operation":"review", "proposal_id":proposal_id, "revision":revision
                }),
            )
            .await;
        assert_eq!(result["outcome"], expected_outcome);
        assert_eq!(result["reason_code"], expected_reason);
        assert_eq!(result["proposal_status"], "submitted");
    }
    let accepted = harness
        .call(
            "art_knowledge_governance",
            json!({
                "operation":"review", "proposal_id":proposal_id, "revision":revision
            }),
        )
        .await;
    assert_eq!(accepted["proposal_status"], "approved");
    harness.stop().await;
}

#[tokio::test]
async fn elicitation_timeout_is_bounded_and_a_later_request_can_retry() {
    let harness = Harness::start_with_timeout(vec![
        ElicitResult::new(ElicitationAction::Accept).with_content(
            json!({"decision":"approve","reason":"Approved after the timed-out request."}),
        ),
    ])
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let timed_out = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(timed_out["outcome"], "operator_action_required");
    assert_eq!(timed_out["reason_code"], "ELICITATION_INTERRUPTED");
    assert_eq!(timed_out["proposal_status"], "submitted");
    let retried = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(retried["proposal_status"], "approved");
    harness.stop().await;
}

#[tokio::test]
async fn wrong_revision_and_publish_replay_fail_without_extra_editions() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":"Exact revision approved."})),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let wrong = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision + 1}),
        )
        .await;
    assert_eq!(wrong["reason_code"], "SOURCE_STALE");
    assert!(harness.requests.lock().unwrap().is_empty());
    let approved = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(approved["proposal_status"], "approved");
    let first = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(first["edition_number"], 1);
    let replay = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(replay["outcome"], "operator_action_required");
    assert_eq!(replay["reason_code"], "INVALID_GOVERNANCE_STATE");
    let rereview = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(rereview["reason_code"], "INVALID_GOVERNANCE_STATE");
    assert_eq!(harness.edition_count(), 1);
    assert_eq!(harness.requests.lock().unwrap().len(), 2);
    harness.stop().await;
}

#[tokio::test]
async fn a_revised_private_source_blocks_review_before_elicitation() {
    let harness = Harness::start(true, vec![]).await;
    let (proposal_id, revision) = harness.proposal().await;
    harness.revise_proposal_source(&proposal_id).await;
    let result = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(result["outcome"], "operator_action_required");
    assert_eq!(result["reason_code"], "SOURCE_STALE");
    assert!(harness.requests.lock().unwrap().is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn concurrent_review_requests_commit_only_one_human_decision() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":"First concurrent review."})),
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"reject","reason":"Second concurrent review."})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let args = json!({"operation":"review", "proposal_id":proposal_id, "revision":revision});
    let (left, right) = tokio::join!(
        harness.call("art_knowledge_governance", args.clone()),
        harness.call("art_knowledge_governance", args),
    );
    let outcomes = [
        left["outcome"].as_str().unwrap(),
        right["outcome"].as_str().unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == "reviewed")
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == "operator_action_required")
            .count(),
        1
    );
    let connection = rusqlite::Connection::open(
        harness
            .root
            .path()
            .join("data/art/knowledge-vault/art-control.sqlite3"),
    )
    .unwrap();
    let reviews: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM proposal_reviews WHERE proposal_id=?1",
            [&proposal_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reviews, 1);
    harness.stop().await;
}

#[tokio::test]
async fn malformed_publish_response_fails_closed_and_can_be_retried() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept).with_content(
                json!({"decision":"approve","reason":"Approved for publish validation."}),
            ),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":"yes"})),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let review = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(review["proposal_status"], "approved");
    let malformed = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(malformed["reason_code"], "ELICITATION_INVALID_RESPONSE");
    assert_eq!(harness.edition_count(), 0);
    let published = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(published["outcome"], "published");
    assert_eq!(harness.edition_count(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn concurrent_publish_confirmations_materialize_exactly_one_edition() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept).with_content(
                json!({"decision":"approve","reason":"Approved before concurrent publication."}),
            ),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let review = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(review["proposal_status"], "approved");
    let args = json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision});
    let (left, right) = tokio::join!(
        harness.call("art_knowledge_governance", args.clone()),
        harness.call("art_knowledge_governance", args),
    );
    let outcomes = [
        left["outcome"].as_str().unwrap(),
        right["outcome"].as_str().unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == "published")
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == "operator_action_required")
            .count(),
        1
    );
    assert_eq!(harness.edition_count(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn elevated_single_source_requires_a_distinct_second_operator() {
    let harness = Harness::start(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({
                "decision":"approve", "reason":"First human approval recorded."
            })),
        ],
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    harness.set_proposal_risk(&proposal_id, "elevated");
    let first = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(first["outcome"], "operator_action_required");
    assert_eq!(first["proposal_status"], "under_review");
    assert_eq!(first["reason_code"], "INDEPENDENT_REVIEW_REQUIRED");
    let second = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(second["reason_code"], "INDEPENDENT_REVIEW_REQUIRED");
    assert_eq!(harness.requests.lock().unwrap().len(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn publish_revalidates_the_approved_snapshot_after_human_confirmation() {
    let harness = Harness::start_with_stale_request(
        true,
        vec![
            ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({"decision":"approve","reason":"Approved exact snapshot."})),
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"confirm":true})),
        ],
        Some(2),
    )
    .await;
    let (proposal_id, revision) = harness.proposal().await;
    let approved = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"review", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(approved["proposal_status"], "approved");

    let publish = harness
        .call(
            "art_knowledge_governance",
            json!({"operation":"publish", "proposal_id":proposal_id, "revision":revision}),
        )
        .await;
    assert_eq!(publish["outcome"], "operator_action_required");
    assert_eq!(publish["reason_code"], "SOURCE_STALE");
    assert_eq!(harness.edition_count(), 0);
    harness.stop().await;
}
