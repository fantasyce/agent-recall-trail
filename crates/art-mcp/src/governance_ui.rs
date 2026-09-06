use std::{collections::BTreeMap, sync::Arc, time::Duration};

use art_domain::{ArtError, ArtResult, agent::AgentId, knowledge::ReviewActor};
use art_knowledge::{DelegationMode, GovernanceSnapshot, KnowledgeVault};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex};
use url::Url;

const SESSION_LIFETIME: Duration = Duration::from_mins(10);
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
        .route("/api/delegation", post(update_delegation))
        .route("/api/review", post(review))
        .route("/api/publish", post(publish))
        .with_state(state)
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
                    "source_set_hash": proposal.source_set_hash,
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
        "proposals": proposals,
        "audit": audit.into_iter().rev().take(100).collect::<Vec<_>>(),
    }))
    .into_response()
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
