//! Thin future Across Context contracts.

use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::{ArtError, ArtResult, agent::AgentId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AccessGrant {
    pub schema: String,
    pub grant_id: String,
    pub owner_agent: AgentId,
    pub target_agent: AgentId,
    pub purpose: String,
    pub source_refs: Vec<String>,
    pub allowed_fields: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub max_uses: u32,
    pub no_persist: bool,
    pub revocation_epoch: u64,
}

#[derive(Debug, Clone)]
pub struct GrantUse {
    pub target_agent: AgentId,
    pub purpose: String,
    pub requested_fields: Vec<String>,
    pub uses_so_far: u32,
    pub observed_revocation_epoch: u64,
    pub now: DateTime<Utc>,
}

impl AccessGrant {
    pub fn authorize(&self, request: &GrantUse) -> ArtResult<()> {
        if self.schema != "art.grant.v1"
            || self.purpose.trim().is_empty()
            || self.source_refs.is_empty()
            || self.allowed_fields.is_empty()
            || !self.no_persist
        {
            return Err(ArtError::InvalidInput("invalid thin-broker grant".into()));
        }
        if request.now >= self.expires_at || request.uses_so_far >= self.max_uses {
            return Err(ArtError::GrantExpired);
        }
        if request.target_agent != self.target_agent
            || request.purpose != self.purpose
            || request.observed_revocation_epoch != self.revocation_epoch
        {
            return Err(ArtError::PermissionDenied(
                "grant target, purpose, or revocation epoch mismatch".into(),
            ));
        }
        if request
            .requested_fields
            .iter()
            .any(|field| !self.allowed_fields.contains(field))
        {
            return Err(ArtError::PermissionDenied(
                "grant field scope exceeded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeliveryReceipt {
    pub receipt_id: String,
    pub grant_id: String,
    pub subject_hashes: Vec<String>,
    pub delivered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextPack {
    pub schema: String,
    pub grant_id: String,
    pub target_agent: AgentId,
    pub excerpts: Vec<String>,
    pub no_persist: bool,
    pub expires_at: DateTime<Utc>,
    pub delivery_receipt: DeliveryReceipt,
}

impl ContextPack {
    pub fn from_grant(
        grant: &AccessGrant,
        excerpts: Vec<String>,
        now: DateTime<Utc>,
    ) -> ArtResult<Self> {
        if now >= grant.expires_at || excerpts.is_empty() {
            return Err(ArtError::GrantExpired);
        }
        let subject_hashes = excerpts
            .iter()
            .map(|excerpt| hex::encode(Sha256::digest(excerpt.as_bytes())))
            .collect();
        Ok(Self {
            schema: "art.context-pack.v1".into(),
            grant_id: grant.grant_id.clone(),
            target_agent: grant.target_agent.clone(),
            excerpts,
            no_persist: true,
            expires_at: grant.expires_at.min(now + Duration::minutes(10)),
            delivery_receipt: DeliveryReceipt {
                receipt_id: format!("artx_{}", Ulid::new()),
                grant_id: grant.grant_id.clone(),
                subject_hashes,
                delivered_at: now,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InvalidationEvent {
    pub schema: String,
    pub subject_ref: String,
    pub new_epoch: u64,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

impl InvalidationEvent {
    pub fn validate_consumer_epoch(&self, observed: u64) -> ArtResult<()> {
        if observed < self.new_epoch {
            Err(ArtError::PermissionDenied(
                "consumer revocation epoch is stale".into(),
            ))
        } else {
            Ok(())
        }
    }
}
