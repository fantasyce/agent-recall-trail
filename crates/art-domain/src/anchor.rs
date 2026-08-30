//! Source anchors and append-only assurance contracts.

use chrono::{DateTime, Utc};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::{
    ArtError, ArtResult,
    agent::AgentId,
    memory::{Sensitivity, canonical_json_hash},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    HostSessionRange,
    UserStatement,
    FileSnapshot,
    GitObject,
    CommandReceipt,
    TestReceipt,
    LogExcerpt,
    ExternalDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceAnchor {
    pub id: String,
    pub owner_agent_id: AgentId,
    pub kind: AnchorKind,
    pub locator: String,
    pub source_version: Option<String>,
    pub source_digest: Option<String>,
    pub excerpt: Option<String>,
    pub excerpt_hash: Option<String>,
    pub metadata: Value,
    pub sensitivity: Sensitivity,
    pub observed_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub content_hash: String,
}

impl SourceAnchor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner_agent_id: AgentId,
        kind: AnchorKind,
        locator: impl Into<String>,
        excerpt: Option<String>,
        metadata: Value,
        sensitivity: Sensitivity,
        observed_at: DateTime<Utc>,
    ) -> ArtResult<Self> {
        Self::new_with_source(
            owner_agent_id,
            kind,
            locator,
            None,
            None,
            excerpt,
            metadata,
            sensitivity,
            observed_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_source(
        owner_agent_id: AgentId,
        kind: AnchorKind,
        locator: impl Into<String>,
        source_version: Option<String>,
        source_digest: Option<String>,
        excerpt: Option<String>,
        metadata: Value,
        sensitivity: Sensitivity,
        observed_at: DateTime<Utc>,
    ) -> ArtResult<Self> {
        let locator = locator.into();
        if locator.trim().is_empty() || locator.contains("..") || locator.len() > 2_048 {
            return Err(ArtError::InvalidInput(
                "unsafe or empty source locator".into(),
            ));
        }
        if excerpt.as_ref().is_some_and(|value| value.len() > 4_096) {
            return Err(ArtError::InvalidInput(
                "source excerpt exceeds 4 KiB".into(),
            ));
        }
        if source_version
            .as_ref()
            .is_some_and(|value| value.len() > 256)
            || source_digest
                .as_ref()
                .is_some_and(|value| value.len() > 256)
        {
            return Err(ArtError::InvalidInput(
                "source version or digest is too large".into(),
            ));
        }
        let secret = Regex::new(r"(?i)(authorization:\s*bearer|begin (rsa|openssh|ec) private key|api[_-]?key\s*=|password\s*=)")
            .map_err(|error| ArtError::Internal(error.to_string()))?;
        if excerpt.as_ref().is_some_and(|value| secret.is_match(value)) {
            return Err(ArtError::InvalidInput(
                "source excerpt contains secret-like material".into(),
            ));
        }
        if matches!(kind, AnchorKind::HostSessionRange)
            && metadata.get("full_transcript").and_then(Value::as_bool) == Some(true)
        {
            return Err(ArtError::InvalidInput(
                "full transcripts are not accepted".into(),
            ));
        }
        if matches!(kind, AnchorKind::CommandReceipt | AnchorKind::TestReceipt)
            && metadata.get("passed").is_some()
            && metadata.get("exit_code").is_none()
            && metadata.get("output_hash").is_none()
        {
            return Err(ArtError::InvalidInput(
                "receipt requires exit code or output hash".into(),
            ));
        }
        let excerpt_hash = excerpt
            .as_ref()
            .map(|value| hex::encode(Sha256::digest(value.as_bytes())));
        let content_hash = canonical_json_hash(&serde_json::json!({
            "kind": kind,
            "locator": locator,
            "source_version":source_version,
            "source_digest":source_digest,
            "excerpt_hash": excerpt_hash,
            "metadata": metadata,
            "sensitivity": sensitivity,
            "observed_at": observed_at,
        }));
        Ok(Self {
            id: format!("arta_{}", Ulid::new()),
            owner_agent_id,
            kind,
            locator,
            source_version,
            source_digest,
            excerpt,
            excerpt_hash,
            metadata,
            sensitivity,
            observed_at,
            verified_at: None,
            content_hash,
        })
    }
}

pub fn anchor_set_hash<'a>(anchors: impl IntoIterator<Item = &'a SourceAnchor>) -> String {
    let mut hashes: Vec<_> = anchors
        .into_iter()
        .map(|anchor| format!("{}:{}", anchor.id, anchor.content_hash))
        .collect();
    hashes.sort_unstable();
    hex::encode(Sha256::digest(hashes.join("\n").as_bytes()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssuranceOutcome {
    Corroborated,
    PartiallyCorroborated,
    Disputed,
    Invalidated,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssuranceDecision {
    pub id: String,
    pub memory_id: String,
    pub memory_revision: u32,
    pub outcome: AssuranceOutcome,
    pub anchor_set_hash: String,
    pub actor: String,
    pub rationale: String,
    pub decided_at: DateTime<Utc>,
}

impl AssuranceDecision {
    pub fn new(
        memory_id: impl Into<String>,
        memory_revision: u32,
        outcome: AssuranceOutcome,
        anchor_set_hash: impl Into<String>,
        actor: impl Into<String>,
        rationale: impl Into<String>,
        decided_at: DateTime<Utc>,
    ) -> ArtResult<Self> {
        let memory_id = memory_id.into();
        let anchor_set_hash = anchor_set_hash.into();
        let actor = actor.into();
        let rationale = rationale.into();
        if memory_id.is_empty()
            || memory_revision == 0
            || anchor_set_hash.len() != 64
            || actor.trim().is_empty()
            || rationale.trim().is_empty()
        {
            return Err(ArtError::InvalidInput(
                "assurance must bind an exact revision, anchor set, actor, and rationale".into(),
            ));
        }
        Ok(Self {
            id: format!("artd_{}", Ulid::new()),
            memory_id,
            memory_revision,
            outcome,
            anchor_set_hash,
            actor,
            rationale,
            decided_at,
        })
    }
}
