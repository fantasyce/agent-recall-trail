use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use art_domain::{
    ArtError, ArtResult,
    agent::AgentId,
    knowledge::{ProposalStatus, ReviewActor, RiskLevel},
};
use art_knowledge::{DelegationMode, GovernanceSnapshot, KnowledgeVault};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use pulldown_cmark::{Options, Parser, html};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use tokio::{net::TcpListener, sync::Mutex};
use url::Url;

const SESSION_LIFETIME: Duration = Duration::from_mins(30);
const INDEX_HTML: &str = include_str!("../assets/governance/index.html");
const STYLES_CSS: &str = include_str!("../assets/governance/styles.css");
const APP_JS: &str = include_str!("../assets/governance/app.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiView {
    Pending,
    Settings,
    Audit,
}

#[derive(Debug, Clone, Serialize)]
pub struct GovernanceUiSession {
    pub url: Url,
    pub origin: String,
    pub expires_at: DateTime<Utc>,
    pub view: UiView,
    pub proposal_id: Option<String>,
    pub revision: Option<u32>,
    #[serde(skip)]
    pub capability: String,
}

#[derive(Debug, Clone)]
struct StoredSession {
    csrf_token: String,
    expires_at: DateTime<Utc>,
    view: UiView,
    proposal_id: Option<String>,
    revision: Option<u32>,
    authorized_proposals: BTreeSet<(String, u32)>,
}

#[derive(Debug)]
struct ServerBinding {
    origin: String,
}

#[derive(Debug, Clone)]
struct AppState {
    vault: KnowledgeVault,
    agent_id: AgentId,
    host_binding_hash: String,
    sessions: Arc<std::sync::RwLock<BTreeMap<String, StoredSession>>>,
    origin: Arc<std::sync::RwLock<Option<String>>>,
}

#[derive(Debug, Clone)]
pub struct GovernanceUiManager {
    state: AppState,
    server: Arc<Mutex<Option<ServerBinding>>>,
}

impl GovernanceUiManager {
    #[must_use]
    pub fn new(vault: KnowledgeVault, agent_id: AgentId, host_binding_hash: String) -> Self {
        Self {
            state: AppState {
                vault,
                agent_id,
                host_binding_hash,
                sessions: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
                origin: Arc::new(std::sync::RwLock::new(None)),
            },
            server: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn open_session(
        &self,
        view: UiView,
        proposal: Option<(String, u32)>,
    ) -> ArtResult<GovernanceUiSession> {
        let origin = self.ensure_server().await?;
        let capability = random_token()?;
        let csrf_token = random_token()?;
        let expires_at = Utc::now()
            + chrono::Duration::from_std(SESSION_LIFETIME)
                .map_err(|error| ArtError::Internal(error.to_string()))?;
        let (proposal_id, revision) =
            proposal.map_or((None, None), |(id, revision)| (Some(id), Some(revision)));
        let authorized_proposals = if let (Some(id), Some(revision)) = (&proposal_id, revision) {
            BTreeSet::from([(id.clone(), revision)])
        } else if view == UiView::Pending {
            self.state
                .vault
                .list_proposals()?
                .into_iter()
                .filter(is_actionable)
                .map(|proposal| (proposal.id, proposal.revision))
                .collect()
        } else {
            BTreeSet::new()
        };
        self.state
            .sessions
            .write()
            .map_err(|_| ArtError::Internal("governance session lock poisoned".into()))?
            .insert(
                capability.clone(),
                StoredSession {
                    csrf_token,
                    expires_at,
                    view,
                    proposal_id: proposal_id.clone(),
                    revision,
                    authorized_proposals,
                },
            );
        let url = Url::parse_with_params(
            &format!("{origin}/review"),
            &[("session", capability.as_str())],
        )
        .map_err(|error| ArtError::Internal(error.to_string()))?;
        Ok(GovernanceUiSession {
            url,
            origin,
            expires_at,
            view,
            proposal_id,
            revision,
            capability,
        })
    }

    async fn ensure_server(&self) -> ArtResult<String> {
        let mut server = self.server.lock().await;
        if let Some(binding) = server.as_ref() {
            return Ok(binding.origin.clone());
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| ArtError::Io(error.to_string()))?;
        let address = listener
            .local_addr()
            .map_err(|error| ArtError::Io(error.to_string()))?;
        let origin = format!("http://127.0.0.1:{}", address.port());
        *self
            .state
            .origin
            .write()
            .map_err(|_| ArtError::Internal("governance origin lock poisoned".into()))? =
            Some(origin.clone());
        let app = router(self.state.clone());
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                tracing::warn!(%error, "ART governance UI server stopped");
            }
        });
        *server = Some(ServerBinding {
            origin: origin.clone(),
        });
        Ok(origin)
    }
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/review", get(index))
        .route("/styles.css", get(styles))
        .route("/app.js", get(script))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/proposal-detail", get(proposal_detail))
        .route("/api/delegation", post(update_delegation))
        .route("/api/review", post(review))
        .route("/api/publish", post(publish))
        .with_state(state)
        .layer(middleware::from_fn(security_headers))
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLES_CSS,
    )
}

async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
}

#[derive(Debug, Deserialize)]
struct SessionQuery {
    session: String,
}

async fn bootstrap(State(state): State<AppState>, Query(query): Query<SessionQuery>) -> Response {
    let session = match authorized_session(&state, &query.session) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let mode = match state
        .vault
        .delegation_mode(&state.agent_id, &state.host_binding_hash)
    {
        Ok(mode) => mode,
        Err(error) => return internal_error_response(&error),
    };
    let proposals = match state.vault.list_proposals() {
        Ok(proposals) => proposals
            .into_iter()
            .filter(is_actionable)
            .filter(|proposal| {
                session
                    .authorized_proposals
                    .contains(&(proposal.id.clone(), proposal.revision))
            })
            .take(100)
            .map(|proposal| {
                json!({
                    "proposal_id": proposal.id,
                    "revision": proposal.revision,
                    "status": proposal.status,
                    "knowledge_key": proposal.draft.knowledge_key,
                    "title": proposal.draft.title,
                    "applicability": proposal.draft.applicability,
                    "sensitivity": proposal.draft.sensitivity,
                    "risk": proposal.draft.risk,
                    "updated_at": proposal.updated_at,
                })
            })
            .collect::<Vec<_>>(),
        Err(error) => return internal_error_response(&error),
    };
    let mut audit = Vec::new();
    for proposal in &proposals {
        let Some(id) = proposal["proposal_id"].as_str() else {
            continue;
        };
        match state.vault.delegated_receipts(id) {
            Ok(receipts) => audit.extend(receipts.into_iter().map(|receipt| {
                json!({
                    "operation": receipt.operation,
                    "proposal_id": receipt.proposal_id,
                    "revision": receipt.proposal_revision,
                    "edition_id": receipt.edition_id,
                    "actor_type": receipt.actor_type,
                    "agent_id": receipt.agent_id,
                    "host_binding": &receipt.host_binding_hash[..12],
                    "created_at": receipt.created_at,
                })
            })),
            Err(error) => return internal_error_response(&error),
        }
    }
    Json(json!({
        "schema": "art.governance.ui.v1",
        "view": session.view,
        "proposal_id": session.proposal_id,
        "revision": session.revision,
        "expires_at": session.expires_at,
        "csrf_token": session.csrf_token,
        "bound_agent_id": state.agent_id.as_str(),
        "host_binding": &state.host_binding_hash[..12],
        "governance_mode": mode,
        "actionable_count": proposals.len(),
        "proposals": proposals,
        "audit": audit.into_iter().rev().take(100).collect::<Vec<_>>(),
    }))
    .into_response()
}

fn is_actionable(proposal: &art_domain::knowledge::KnowledgeProposal) -> bool {
    matches!(
        proposal.status,
        ProposalStatus::Submitted | ProposalStatus::UnderReview | ProposalStatus::Approved
    )
}

#[derive(Debug, Deserialize)]
struct ProposalDetailQuery {
    session: String,
    proposal_id: String,
    revision: u32,
}

async fn proposal_detail(
    State(state): State<AppState>,
    Query(query): Query<ProposalDetailQuery>,
) -> Response {
    let session = match authorized_session(&state, &query.session) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    if !session
        .authorized_proposals
        .contains(&(query.proposal_id.clone(), query.revision))
    {
        return StatusCode::CONFLICT.into_response();
    }
    let proposal = match state.vault.proposal(&query.proposal_id) {
        Ok(proposal) => proposal,
        Err(error) => return art_error_response(&error),
    };
    if proposal.revision != query.revision {
        return StatusCode::CONFLICT.into_response();
    }
    let reviews = match state
        .vault
        .proposal_reviews(&proposal.id, proposal.revision)
    {
        Ok(reviews) => reviews,
        Err(error) => return art_error_response(&error),
    };
    let current = match state.vault.verified_current(&proposal.draft.knowledge_key) {
        Ok(current) => current,
        Err(error) => return art_error_response(&error),
    };
    let predicted_edition_number = match state
        .vault
        .next_edition_number(&proposal.draft.knowledge_key)
    {
        Ok(number) => number,
        Err(error) => return art_error_response(&error),
    };
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    let canonical_markdown = canonical_review_markdown(
        &proposal.draft.applicability,
        &proposal.draft.markdown,
    );
    let rendered_html = render_markdown(&canonical_markdown);
    let (current_edition, comparison) = current.map_or_else(
        || {
            (
                serde_json::Value::Null,
                json!({
                    "kind": "first_edition",
                    "message": "This is the first Edition; all proposal content is new.",
                    "groups": [],
                }),
            )
        },
        |verified| {
            let comparison = line_diff(&verified.canonical_markdown, &canonical_markdown);
            let record = verified.record;
            (
                json!({
                    "edition_id": record.edition_id,
                    "edition_number": record.edition_number,
                    "title": record.title,
                    "published_at": record.published_at,
                    "markdown_sha256": record.markdown_sha256,
                    "manifest_sha256": record.manifest_sha256,
                    "canonical_markdown": verified.canonical_markdown,
                }),
                json!({"kind":"line_diff","context_lines":3,"groups":comparison}),
            )
        },
    );
    let needs_independent_review = matches!(proposal.draft.risk, RiskLevel::Elevated | RiskLevel::High)
        && proposal
            .sources
            .iter()
            .map(|source| &source.source_content_hash)
            .collect::<BTreeSet<_>>()
            .len()
            < 2
        && proposal.status == ProposalStatus::UnderReview;
    let allowed_actions: Vec<&str> = match proposal.status {
        ProposalStatus::Submitted => vec!["approved", "changes_requested", "rejected"],
        ProposalStatus::UnderReview => vec!["changes_requested", "rejected"],
        ProposalStatus::Approved => vec!["publish"],
        _ => Vec::new(),
    };
    Json(json!({
        "schema": "art.governance.proposal-detail.v1",
        "proposal": {
            "proposal_id": proposal.id,
            "revision": proposal.revision,
            "status": proposal.status,
            "knowledge_key": proposal.draft.knowledge_key,
            "title": proposal.draft.title,
            "sensitivity": proposal.draft.sensitivity,
            "risk": proposal.draft.risk,
            "author_agent_id": proposal.author_agent_id,
            "created_at": proposal.created_at,
            "updated_at": proposal.updated_at,
            "draft_hash": snapshot.draft_hash,
            "source_set_hash": proposal.source_set_hash,
        },
        "content": {
            "applicability_markdown": proposal.draft.applicability,
            "knowledge_markdown": proposal.draft.markdown,
            "canonical_markdown": canonical_markdown,
            "rendered_html": rendered_html,
        },
        "sources": proposal.sources,
        "reviews": reviews,
        "current_edition": current_edition,
        "comparison": comparison,
        "predicted_edition_number": predicted_edition_number,
        "allowed_actions": allowed_actions,
        "requirements": {
            "independent_review_required": needs_independent_review,
            "can_satisfy_in_current_session": !needs_independent_review,
            "message": if needs_independent_review {
                "A distinct reviewer is required; this local session cannot claim a second identity."
            } else {
                "No additional review is currently required."
            },
        },
    }))
    .into_response()
}

fn canonical_review_markdown(applicability: &str, knowledge: &str) -> String {
    format!(
        "## Applicability\n\n{}\n\n## Knowledge\n\n{}",
        applicability.trim(),
        knowledge.trim()
    )
}

fn render_markdown(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, options);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    ammonia::Builder::default()
        .rm_tags(["img"])
        .link_rel(Some("noopener noreferrer"))
        .clean(&rendered)
        .to_string()
}

fn line_diff(previous: &str, proposed: &str) -> Vec<Vec<serde_json::Value>> {
    let diff = TextDiff::from_lines(previous, proposed);
    diff.grouped_ops(3)
        .iter()
        .map(|group| {
            group
                .iter()
                .flat_map(|operation| diff.iter_changes(operation))
                .map(|change| {
                    let (kind, label) = match change.tag() {
                        ChangeTag::Delete => ("removed", "Removed"),
                        ChangeTag::Insert => ("added", "Added"),
                        ChangeTag::Equal => ("unchanged", "Unchanged"),
                    };
                    json!({
                        "kind": kind,
                        "label": label,
                        "old_line": change.old_index().map(|index| index + 1),
                        "new_line": change.new_index().map(|index| index + 1),
                        "text": change.value().trim_end_matches('\n'),
                    })
                })
                .collect()
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct DelegationRequest {
    session: String,
    mode: DelegationMode,
}

async fn update_delegation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DelegationRequest>,
) -> Response {
    if let Err(response) = authorized_mutation(&state, &headers, &request.session, None) {
        return response.into_response();
    }
    match state.vault.set_delegation_mode(
        &state.agent_id,
        &state.host_binding_hash,
        request.mode,
        "local_governance_ui",
    ) {
        Ok(()) => Json(json!({"ok":true,"governance_mode":request.mode})).into_response(),
        Err(error) => internal_error_response(&error),
    }
}

#[derive(Debug, Deserialize)]
struct ReviewRequest {
    session: String,
    proposal_id: String,
    revision: u32,
    decision: String,
    reason: String,
}

async fn review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReviewRequest>,
) -> Response {
    if let Err(response) = authorized_mutation(
        &state,
        &headers,
        &request.session,
        Some((&request.proposal_id, request.revision)),
    ) {
        return response.into_response();
    }
    let proposal = match state.vault.proposal(&request.proposal_id) {
        Ok(proposal) => proposal,
        Err(error) => return art_error_response(&error),
    };
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    match state.vault.review_exact(
        &snapshot,
        ReviewActor::Human("local-governance-ui".into()),
        &request.decision,
        &request.reason,
    ) {
        Ok(()) => Json(json!({"ok":true,"outcome":"reviewed"})).into_response(),
        Err(error) => art_error_response(&error),
    }
}

#[derive(Debug, Deserialize)]
struct PublishRequest {
    session: String,
    proposal_id: String,
    revision: u32,
    confirm: bool,
}

async fn publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PublishRequest>,
) -> Response {
    if let Err(response) = authorized_mutation(
        &state,
        &headers,
        &request.session,
        Some((&request.proposal_id, request.revision)),
    ) {
        return response.into_response();
    }
    let proposal = match state.vault.proposal(&request.proposal_id) {
        Ok(proposal) => proposal,
        Err(error) => return art_error_response(&error),
    };
    let snapshot = GovernanceSnapshot::from_proposal(&proposal);
    let next = match state
        .vault
        .next_edition_number(&proposal.draft.knowledge_key)
    {
        Ok(next) => next,
        Err(error) => return art_error_response(&error),
    };
    match state.vault.publish_exact(&snapshot, next, request.confirm) {
        Ok(edition) => Json(json!({
            "ok":true,
            "outcome":"published",
            "edition_id":edition.edition_id,
            "edition_number":edition.edition_number,
        }))
        .into_response(),
        Err(error) => art_error_response(&error),
    }
}

fn authorized_session(state: &AppState, capability: &str) -> Result<StoredSession, StatusCode> {
    let mut sessions = state
        .sessions
        .write()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sessions.retain(|_, session| session.expires_at > Utc::now());
    sessions
        .get(capability)
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)
}

fn authorized_mutation(
    state: &AppState,
    headers: &HeaderMap,
    capability: &str,
    proposal: Option<(&str, u32)>,
) -> Result<StoredSession, StatusCode> {
    let session = authorized_session(state, capability)?;
    let origin = state
        .origin
        .read()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != origin.as_deref()
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if headers
        .get("x-art-csrf")
        .and_then(|value| value.to_str().ok())
        != Some(session.csrf_token.as_str())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if let (Some(expected_id), Some(expected_revision), Some((id, revision))) =
        (session.proposal_id.as_deref(), session.revision, proposal)
        && (expected_id != id || expected_revision != revision)
    {
        return Err(StatusCode::CONFLICT);
    }
    Ok(session)
}

fn art_error_response(error: &ArtError) -> Response {
    let status = match error {
        ArtError::NotFound => StatusCode::NOT_FOUND,
        ArtError::PermissionDenied(_) | ArtError::IdentityMismatch => StatusCode::FORBIDDEN,
        ArtError::SourceStale | ArtError::InvalidStateTransition => StatusCode::CONFLICT,
        ArtError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error":error.code()}))).into_response()
}

fn internal_error_response(error: &ArtError) -> Response {
    art_error_response(error)
}

fn random_token() -> ArtResult<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| ArtError::Internal(error.to_string()))?;
    Ok(hex::encode(bytes))
}
