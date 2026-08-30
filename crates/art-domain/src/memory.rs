//! Typed private memory contracts.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::{ArtError, ArtResult, agent::AgentId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Private,
    Internal,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "key", rename_all = "snake_case")]
pub enum MemoryScope {
    Session(String),
    Repository(String),
    Workspace(String),
    Machine(String),
    User(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Candidate,
    Active,
    Disputed,
    Superseded,
    Rejected,
    Archived,
}

impl MemoryStatus {
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Candidate,
                Self::Active | Self::Rejected | Self::Archived
            ) | (
                Self::Active,
                Self::Disputed | Self::Superseded | Self::Archived
            ) | (
                Self::Disputed,
                Self::Active | Self::Superseded | Self::Archived
            ) | (Self::Superseded | Self::Rejected, Self::Archived)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodePayload {
    pub situation: String,
    pub actions: Vec<String>,
    pub outcome: String,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticPayload {
    pub statement: String,
    pub applicability: String,
    #[serde(default)]
    pub exceptions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcedurePayload {
    pub prerequisites: Vec<String>,
    pub steps: Vec<String>,
    pub verification: Vec<String>,
    pub rollback: Vec<String>,
    pub do_not_use_when: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecisionPayload {
    pub decision: String,
    pub rationale: String,
    pub alternatives: Vec<String>,
    pub accepted_risks: Vec<String>,
    pub revisit_when: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum MemoryPayload {
    Episode(EpisodePayload),
    Semantic(SemanticPayload),
    Procedure(ProcedurePayload),
    Decision(DecisionPayload),
}

impl MemoryPayload {
    pub fn validate(&self) -> ArtResult<()> {
        let fields: Vec<&str> = match self {
            Self::Episode(value) => vec![value.situation.as_str(), value.outcome.as_str()]
                .into_iter()
                .chain(value.actions.iter().map(String::as_str))
                .chain(value.open_questions.iter().map(String::as_str))
                .collect(),
            Self::Semantic(value) => vec![&value.statement, &value.applicability],
            Self::Procedure(value) => value
                .prerequisites
                .iter()
                .chain(&value.steps)
                .chain(&value.verification)
                .chain(&value.rollback)
                .chain(&value.do_not_use_when)
                .map(String::as_str)
                .collect(),
            Self::Decision(value) => std::iter::once(value.decision.as_str())
                .chain(std::iter::once(value.rationale.as_str()))
                .chain(value.alternatives.iter().map(String::as_str))
                .chain(value.accepted_risks.iter().map(String::as_str))
                .collect(),
        };
        if fields.is_empty()
            || fields
                .iter()
                .any(|field| field.trim().is_empty() || field.len() > 16_384)
        {
            return Err(ArtError::InvalidInput(
                "memory payload has an empty or oversized required field".into(),
            ));
        }
        match self {
            Self::Procedure(value)
                if value.prerequisites.is_empty()
                    || value.steps.is_empty()
                    || value.verification.is_empty()
                    || value.rollback.is_empty()
                    || value.do_not_use_when.is_empty() =>
            {
                Err(ArtError::InvalidInput(
                    "procedure requires prerequisites, steps, verification, rollback, and exclusions".into(),
                ))
            }
            Self::Decision(value)
                if value.alternatives.is_empty() || value.accepted_risks.is_empty() =>
            {
                Err(ArtError::InvalidInput(
                    "decision requires alternatives and accepted risks".into(),
                ))
            }
            _ => Ok(()),
        }
    }

    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Episode(_) => "episode",
            Self::Semantic(_) => "semantic",
            Self::Procedure(_) => "procedure",
            Self::Decision(_) => "decision",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRevision {
    pub revision: u32,
    pub payload: MemoryPayload,
    pub content_hash: String,
    pub reason: String,
    pub changed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryArtifact {
    pub id: String,
    pub agent_id: AgentId,
    pub title: String,
    pub summary: String,
    pub payload: MemoryPayload,
    pub scope: MemoryScope,
    pub sensitivity: Sensitivity,
    pub status: MemoryStatus,
    pub current_revision: u32,
    pub current_hash: String,
    pub revisions: Vec<MemoryRevision>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub review_after: Option<DateTime<Utc>>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MemoryArtifact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_id: AgentId,
        title: impl Into<String>,
        summary: impl Into<String>,
        payload: MemoryPayload,
        scope: MemoryScope,
        sensitivity: Sensitivity,
        now: DateTime<Utc>,
    ) -> ArtResult<Self> {
        payload.validate()?;
        let title = title.into();
        let summary = summary.into();
        if title.trim().is_empty() || summary.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "title and summary are required".into(),
            ));
        }
        let current_hash = canonical_json_hash(
            &serde_json::to_value(&payload)
                .map_err(|error| ArtError::Internal(error.to_string()))?,
        );
        let revision = MemoryRevision {
            revision: 1,
            payload: payload.clone(),
            content_hash: current_hash.clone(),
            reason: "capture".into(),
            changed_at: now,
        };
        Ok(Self {
            id: format!("artm_{}", Ulid::new()),
            agent_id,
            title,
            summary,
            payload,
            scope,
            sensitivity,
            status: MemoryStatus::Candidate,
            current_revision: 1,
            current_hash,
            revisions: vec![revision],
            valid_from: None,
            valid_until: None,
            review_after: None,
            superseded_by: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn revise(
        &mut self,
        payload: MemoryPayload,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> ArtResult<()> {
        payload.validate()?;
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(ArtError::InvalidInput("revision reason is required".into()));
        }
        let revision = self
            .current_revision
            .checked_add(1)
            .ok_or_else(|| ArtError::InvalidInput("revision overflow".into()))?;
        let content_hash = canonical_json_hash(
            &serde_json::to_value(&payload)
                .map_err(|error| ArtError::Internal(error.to_string()))?,
        );
        self.payload = payload.clone();
        self.current_revision = revision;
        self.current_hash.clone_from(&content_hash);
        self.updated_at = now;
        self.revisions.push(MemoryRevision {
            revision,
            payload,
            content_hash,
            reason,
            changed_at: now,
        });
        Ok(())
    }

    pub fn transition(&mut self, next: MemoryStatus, now: DateTime<Utc>) -> ArtResult<()> {
        if !self.status.can_transition_to(next) {
            return Err(ArtError::InvalidStateTransition);
        }
        self.status = next;
        self.updated_at = now;
        Ok(())
    }
}

pub fn canonical_json_hash(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let mut keys: Vec<_> = object.keys().collect();
                keys.sort_unstable();
                let mut result = Map::new();
                for key in keys {
                    result.insert(key.clone(), canonical(&object[key]));
                }
                Value::Object(result)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    let bytes = serde_json::to_vec(&canonical(value)).expect("JSON values always serialize");
    hex::encode(Sha256::digest(bytes))
}
