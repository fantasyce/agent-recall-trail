//! Knowledge proposal and immutable edition contracts.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    agent::AgentId,
    memory::{Sensitivity, canonical_json_hash},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSourceType {
    PrivateMemory,
    FileSnapshot,
    GitObject,
    TestReceipt,
    ExternalDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProposalSourceLock {
    pub source_type: ProposalSourceType,
    pub owner_agent_id: Option<AgentId>,
    pub source_id: String,
    pub source_revision: Option<u32>,
    pub source_content_hash: String,
    pub anchor_set_hash: Option<String>,
    pub approved_excerpt_hash: Option<String>,
    pub use_grant_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Submitted,
    UnderReview,
    ChangesRequested,
    Approved,
    Rejected,
    Stale,
    Materialized,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Normal,
    Elevated,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnowledgeDraft {
    pub knowledge_key: String,
    pub title: String,
    pub applicability: String,
    pub markdown: String,
    pub sensitivity: Sensitivity,
    pub risk: RiskLevel,
}

impl KnowledgeDraft {
    pub fn minimal(
        key: impl Into<String>,
        title: impl Into<String>,
        markdown: impl Into<String>,
    ) -> Self {
        Self {
            knowledge_key: key.into(),
            title: title.into(),
            applicability: "local coding agents".into(),
            markdown: markdown.into(),
            sensitivity: Sensitivity::Internal,
            risk: RiskLevel::Normal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnowledgeProposal {
    pub id: String,
    pub revision: u32,
    pub status: ProposalStatus,
    pub author_agent_id: AgentId,
    pub draft: KnowledgeDraft,
    pub sources: Vec<ProposalSourceLock>,
    pub source_set_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ReviewActor {
    Human(String),
    Agent(AgentId),
    Policy(String),
}

pub fn proposal_source_set_hash(sources: &[ProposalSourceLock]) -> String {
    let mut values: Vec<_> = sources
        .iter()
        .map(|source| {
            canonical_json_hash(&serde_json::to_value(source).expect("source locks serialize"))
        })
        .collect();
    values.sort_unstable();
    canonical_json_hash(&serde_json::json!(values))
}
