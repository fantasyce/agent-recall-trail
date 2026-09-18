//! One policy boundary for explicit, ordinary-Agent, and Hook memory intake.
use super::{
    AUTO_MEMORY_POLICY_VERSION, AgentId, AgentVault, ArtError, ArtResult, AssuranceDecision,
    AssuranceOutcome, AutoMemoryConfigStore, AutoMemoryOutcome, AutoMemoryStatus,
    AutoMemorySubmission, Connection, DateTime, MemoryArtifact, MemoryStatus, OpenOptions,
    OptionalExtension, Path, Regex, SCHEMA_VERSION, SourceAnchor, Transaction, TransactionBehavior,
    Utc, anchor_set_hash, auto_memory_contains_sensitive, capture_payload_hash,
    evaluate_auto_memory_evidence, insert_auto_submission_receipt, insert_memory_rows, map_db,
    open_connection, params, read_in_transaction, scope_key, scope_type, search_document,
    set_private_permissions, stable_anchor_hashes, update_artifact,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

pub const MEMORY_INTAKE_RECEIPT_SCHEMA: &str = "art.memory-intake.receipt.v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeOrigin {
    UserRequested,
    #[default]
    AgentInitiated,
    HookTriggered,
    LegacyUnspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeDisposition {
    Activated,
    PendingReview,
    Duplicate,
    Rejected,
    Disabled,
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeAttribution {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub host_supplied: bool,
    pub degraded: bool,
}

impl IntakeAttribution {
    pub fn agent_asserted(session_id: Option<String>, turn_id: Option<String>) -> Self {
        Self {
            session_id,
            turn_id,
            host_supplied: false,
            degraded: true,
        }
    }

    pub fn host_supplied(session_id: String, turn_id: Option<String>) -> Self {
        Self {
            session_id: Some(session_id),
            turn_id,
            host_supplied: true,
            degraded: false,
        }
    }

    pub(super) fn bucket(&self, now: DateTime<Utc>) -> String {
        if self.host_supplied
            && let Some(session) = &self.session_id
        {
            return format!("session:{session}");
        }
        format!("agent-day:{}", now.format("%Y-%m-%d"))
    }
}

#[derive(Debug, Clone)]
pub struct MemoryIntakeRequest {
    pub capture_origin: Option<IntakeOrigin>,
    pub memory: MemoryArtifact,
    pub anchors: Vec<SourceAnchor>,
    pub idempotency_key: String,
    pub attribution: IntakeAttribution,
    pub request_basis: Option<String>,
    pub value_reason: Option<String>,
    pub target_memory_id: Option<String>,
    pub expected_revision: Option<u32>,
    pub hook_trigger_receipt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryIntakeReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub origin: IntakeOrigin,
    pub attribution: IntakeAttribution,
    pub payload_hash: String,
    pub evidence_hash: String,
    pub budget_bucket: String,
    pub budget_association: Option<String>,
    pub disposition: IntakeDisposition,
    pub memory_id: Option<String>,
    pub proposal_id: Option<String>,
    pub reason: String,
    pub request_basis: Option<String>,
    pub value_reason: Option<String>,
    pub policy_version: String,
    pub config_version: u64,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryIntakeResult {
    pub receipt_id: String,
    pub origin: IntakeOrigin,
    pub disposition: IntakeDisposition,
    pub memory_id: Option<String>,
    pub proposal_id: Option<String>,
    pub reason: String,
    pub policy_version: String,
    pub config_version: u64,
    pub replayed: bool,
    pub receipt: MemoryIntakeReceipt,
}

impl MemoryIntakeReceipt {
    fn result(self, replayed: bool) -> MemoryIntakeResult {
        MemoryIntakeResult {
            receipt_id: self.receipt_id.clone(),
            origin: self.origin,
            disposition: self.disposition,
            memory_id: self.memory_id.clone(),
            proposal_id: self.proposal_id.clone(),
            reason: self.reason.clone(),
            policy_version: self.policy_version.clone(),
            config_version: self.config_version,
            replayed,
            receipt: self,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRevisionProposal {
    pub proposal_id: String,
    pub target_memory_id: String,
    pub expected_revision: u32,
    pub artifact: MemoryArtifact,
    pub anchors: Vec<SourceAnchor>,
    pub receipt: MemoryIntakeReceipt,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryReviewAction {
    Confirm,
    EditConfirm,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRevisionReview {
    pub proposal_id: String,
    pub expected_revision: u32,
    pub action: MemoryReviewAction,
    pub reason: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub payload: Option<art_domain::memory::MemoryPayload>,
}

impl AgentVault {
    /// Only authenticated human governance adapters may call this operation.
    pub fn review_memory_revision(
        &self,
        review: MemoryRevisionReview,
        actor: &str,
    ) -> ArtResult<serde_json::Value> {
        if !actor.starts_with("human:")
            || review.reason.trim().is_empty()
            || review.reason.len() > 1_000
        {
            return Err(ArtError::InvalidInput(
                "human review requires a bounded reason".into(),
            ));
        }
        let mut connection = open_connection(&self.path)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let json: Option<String> = tx.query_row("SELECT proposal_json FROM memory_revision_proposals WHERE agent_id=?1 AND proposal_id=?2 AND status='pending'",params![self.agent_id.as_str(),review.proposal_id],|row|row.get(0)).optional().map_err(map_db)?;
        let proposal: MemoryRevisionProposal = decode(&json.ok_or(ArtError::SourceStale)?)?;
        let mut current = read_in_transaction(&tx, &self.agent_id, &proposal.target_memory_id)?;
        if review.expected_revision != proposal.expected_revision {
            return Err(ArtError::SourceStale);
        }
        let rejected = matches!(review.action, MemoryReviewAction::Reject);
        if !rejected {
            if current.current_revision != proposal.expected_revision
                || current.status != MemoryStatus::Active
            {
                return Err(ArtError::SourceStale);
            }
            let (title, summary, payload) =
                if matches!(review.action, MemoryReviewAction::EditConfirm) {
                    match (review.title, review.summary, review.payload) {
                        (Some(title), Some(summary), Some(payload)) => (title, summary, payload),
                        _ => {
                            return Err(ArtError::InvalidInput(
                                "edit_confirm requires title, summary, and payload".into(),
                            ));
                        }
                    }
                } else {
                    (
                        proposal.artifact.title.clone(),
                        proposal.artifact.summary.clone(),
                        proposal.artifact.payload.clone(),
                    )
                };
            if title.trim().is_empty() || summary.trim().is_empty() {
                return Err(ArtError::InvalidInput(
                    "title and summary are required".into(),
                ));
            }
            current.title = title.trim().into();
            current.summary = summary.trim().into();
            current.revise(payload, review.reason.trim(), Utc::now())?;
            if auto_memory_contains_sensitive(&current, &proposal.anchors)? {
                return Err(ArtError::InvalidInput(
                    "sensitive revision content rejected".into(),
                ));
            }
            store_revision(&tx, &self.agent_id, &current, &proposal.anchors)?;
            tx.execute(
                "UPDATE memory_revisions SET changed_by=?1 WHERE memory_id=?2 AND revision=?3",
                params![actor, current.id, current.current_revision],
            )
            .map_err(map_db)?;
            let decision = AssuranceDecision::new(
                &current.id,
                current.current_revision,
                AssuranceOutcome::Corroborated,
                anchor_set_hash(&proposal.anchors),
                actor,
                review.reason.trim(),
                Utc::now(),
            )?;
            tx.execute("INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,'corroborated',?4,'human',?5,?6,?7)",params![decision.id,current.id,current.current_revision,decision.anchor_set_hash,actor,review.reason.trim(),decision.decided_at.to_rfc3339()]).map_err(map_db)?;
        }
        let receipt = serde_json::json!({"schema":"art.memory-revision.review.v1","ok":true,"proposal_id":proposal.proposal_id,"memory_id":current.id,"expected_revision":review.expected_revision,"revision":current.current_revision,"status":if rejected {"rejected"} else {"confirmed"},"action":review.action,"actor":actor,"reason":review.reason.trim(),"reviewed_at":Utc::now()});
        let mut record =
            serde_json::to_value(&proposal).map_err(|e| ArtError::Internal(e.to_string()))?;
        record["review"] = receipt.clone();
        tx.execute("UPDATE memory_revision_proposals SET status=?1,proposal_json=?2 WHERE agent_id=?3 AND proposal_id=?4",params![if rejected {"rejected"} else {"confirmed"},encode(&record)?,self.agent_id.as_str(),proposal.proposal_id]).map_err(map_db)?;
        tx.commit().map_err(map_db)?;
        Ok(receipt)
    }

    pub fn intake(
        &self,
        config: &AutoMemoryConfigStore,
        mut input: MemoryIntakeRequest,
    ) -> ArtResult<MemoryIntakeResult> {
        let origin = input.capture_origin.unwrap_or_default();
        // Identity binding applies even to historical replay. New admission
        // rules are checked after exact legacy receipts have been resolved.
        if input.memory.agent_id != self.agent_id
            || input
                .anchors
                .iter()
                .any(|anchor| anchor.owner_agent_id != self.agent_id)
        {
            return Err(ArtError::IdentityMismatch);
        }
        let automatic = origin != IntakeOrigin::UserRequested;
        let initial = config.status();
        let now = Utc::now();
        let payload_hash = art_domain::memory::canonical_json_hash(&serde_json::json!({
            "content": capture_payload_hash(&input.memory, &input.anchors),
            "target_memory_id": input.target_memory_id,
            "expected_revision": input.expected_revision,
        }));
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let prior: Option<(String,String)> = transaction.query_row(
            "SELECT payload_hash,receipt_json FROM memory_intake_receipts WHERE agent_id=?1 AND idempotency_key=?2",
            params![self.agent_id.as_str(),input.idempotency_key], |row|Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(map_db)?;
        if let Some((hash, json)) = prior {
            let prior = decode::<MemoryIntakeReceipt>(&json)?;
            if prior.origin != IntakeOrigin::LegacyUnspecified {
                validate_request(&input, origin, &self.agent_id)?;
            }
            if hash != payload_hash
                && !legacy_capture_matches(&transaction, &self.agent_id, &input, &prior)?
            {
                return Err(ArtError::DuplicateConflict);
            }
            return Ok(prior.result(true));
        }
        validate_request(&input, origin, &self.agent_id)?;
        // Serialize admission and commit with completed governance toggles. No
        // automatic writes can land after a disable returns to its caller.
        let _guard = config.mutation_guard()?;
        let status = config.status();
        input.attribution.degraded =
            !input.attribution.host_supplied || input.attribution.session_id.is_none();
        let mut receipt = MemoryIntakeReceipt {
            schema: MEMORY_INTAKE_RECEIPT_SCHEMA.into(),
            receipt_id: format!("artir_{}", ulid::Ulid::new()),
            origin,
            attribution: input.attribution.clone(),
            payload_hash,
            evidence_hash: art_domain::memory::canonical_json_hash(&serde_json::json!(
                stable_anchor_hashes(&input.anchors)
            )),
            budget_bucket: input.attribution.bucket(now),
            budget_association: None,
            disposition: IntakeDisposition::PendingReview,
            memory_id: None,
            proposal_id: None,
            reason: String::new(),
            request_basis: None,
            value_reason: None,
            policy_version: AUTO_MEMORY_POLICY_VERSION.into(),
            config_version: status.config_version,
            received_at: now.to_rfc3339(),
        };
        if automatic && (!initial.enabled || !status.enabled) {
            receipt.disposition = IntakeDisposition::Disabled;
            receipt.reason = "global_disabled".into();
            return finish(transaction, &self.agent_id, &input, &receipt);
        }
        if origin == IntakeOrigin::HookTriggered {
            let trigger_id = input.hook_trigger_receipt_id.as_deref().ok_or_else(|| {
                ArtError::InvalidInput("Hook intake requires its exact trigger receipt".into())
            })?;
            let trigger: Option<(String,String,String,u64)> = transaction.query_row(
                "SELECT outcome,session_id,turn_id,config_version FROM auto_memory_trigger_receipts WHERE receipt_id=?1 AND agent_id=?2",
                params![trigger_id,self.agent_id.as_str()], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).optional().map_err(map_db)?;
            let Some((outcome, session, turn, version)) = trigger else {
                return Err(ArtError::PermissionDenied(
                    "accepted Hook trigger required".into(),
                ));
            };
            if outcome != "accepted" || version != status.config_version {
                return Err(ArtError::PermissionDenied(
                    "current accepted Hook trigger required".into(),
                ));
            }
            let admission: Option<(Option<String>, Option<String>)> = transaction.query_row(
                "SELECT session_id,turn_id FROM memory_intake_budget WHERE admission_id=?1 AND agent_id=?2",
                params![trigger_id,self.agent_id.as_str()], |row|Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(map_db)?;
            receipt.attribution = match admission {
                Some((Some(session), turn)) => IntakeAttribution::host_supplied(session, turn),
                Some((None, _)) => IntakeAttribution::agent_asserted(None, None),
                None => IntakeAttribution::host_supplied(session, Some(turn)),
            };
            receipt.budget_bucket = receipt.attribution.bucket(now);
            receipt.budget_association = Some(trigger_id.into());
            // The exact trigger is a second idempotency boundary, independent
            // of a caller-selected key.
            let used: Option<(String,String)> = transaction.query_row(
                "SELECT payload_hash,receipt_json FROM memory_intake_receipts WHERE agent_id=?1 AND trigger_receipt_id=?2",
                params![self.agent_id.as_str(),trigger_id], |row|Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(map_db)?;
            if let Some((hash, json)) = used {
                if hash != receipt.payload_hash {
                    return Err(ArtError::DuplicateConflict);
                }
                return Ok(decode::<MemoryIntakeReceipt>(&json)?.result(true));
            }
        }
        if auto_memory_contains_sensitive(&input.memory, &input.anchors)?
            || sensitive_note(input.request_basis.as_deref())?
            || sensitive_note(input.value_reason.as_deref())?
        {
            receipt.disposition = IntakeDisposition::Rejected;
            receipt.reason = "sensitive_content".into();
            return finish(transaction, &self.agent_id, &input, &receipt);
        }
        receipt.request_basis.clone_from(&input.request_basis);
        receipt.value_reason.clone_from(&input.value_reason);
        let target = if let Some(id) = &input.target_memory_id {
            let current = read_in_transaction(&transaction, &self.agent_id, id)?;
            if Some(current.current_revision) != input.expected_revision {
                return Err(ArtError::SourceStale);
            }
            if current.scope != input.memory.scope || current.status != MemoryStatus::Active {
                return Err(ArtError::InvalidInput(
                    "revision requires the same scope and an Active target".into(),
                ));
            }
            Some(current)
        } else {
            None
        };
        let duplicate: Option<String> = transaction.query_row(
            "SELECT id FROM memory_artifacts WHERE agent_id=?1 AND current_hash=?2 AND scope_type=?3 AND scope_key=?4 AND status IN ('active','candidate') ORDER BY updated_at DESC LIMIT 1",
            params![self.agent_id.as_str(),input.memory.current_hash,scope_type(&input.memory.scope),scope_key(&input.memory.scope)],|row|row.get(0),
        ).optional().map_err(map_db)?;
        if let Some(id) = duplicate {
            if origin == IntakeOrigin::HookTriggered {
                // A claim reserves an admission before the candidate exists.
                // Refund an unused reservation once an exact duplicate is known;
                // retain it if a reliably linked ordinary submission used it.
                transaction.execute(
                    "DELETE FROM memory_intake_budget WHERE admission_id=?1 AND NOT EXISTS(SELECT 1 FROM memory_intake_receipts WHERE json_extract(receipt_json,'$.budget_association')=?1 AND disposition IN ('activated','pending_review'))",
                    [&receipt.budget_association],
                ).map_err(map_db)?;
                receipt.budget_association = transaction.query_row(
                    "SELECT json_extract(receipt_json,'$.budget_association') FROM memory_intake_receipts WHERE agent_id=?1 AND memory_id=?2 AND disposition IN ('activated','pending_review') ORDER BY received_at DESC LIMIT 1",
                    params![self.agent_id.as_str(),id], |row|row.get::<_,Option<String>>(0),
                ).optional().map_err(map_db)?.flatten();
            }
            receipt.disposition = IntakeDisposition::Duplicate;
            receipt.reason = "duplicate_existing_memory".into();
            receipt.memory_id = Some(id);
            return finish(transaction, &self.agent_id, &input, &receipt);
        }
        if automatic
            && let Some(id) = &receipt.budget_association
            && admission_consumed(&transaction, &self.agent_id, id)?
        {
            receipt.budget_association = None;
        }
        if automatic && receipt.budget_association.is_none() {
            let linked = unused_hook_admission(
                &transaction,
                &self.agent_id,
                &receipt.attribution,
                status.config_version,
            )?;
            if let Some(id) = linked {
                receipt.budget_association = Some(id);
            } else if let Some(reason) = budget_limit(
                &transaction,
                &self.agent_id,
                &receipt.budget_bucket,
                &status,
                now,
            )? {
                receipt.disposition = IntakeDisposition::RateLimited;
                receipt.reason = reason.into();
                return finish(transaction, &self.agent_id, &input, &receipt);
            } else {
                receipt.budget_association = Some(receipt.receipt_id.clone());
                admit(
                    &transaction,
                    &self.agent_id,
                    &receipt.receipt_id,
                    &receipt.budget_bucket,
                    &receipt.attribution,
                    now,
                )?;
            }
        }
        let conflict: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_artifacts WHERE agent_id=?1 AND lower(title)=lower(?2) AND scope_type=?3 AND scope_key=?4 AND status IN ('active','candidate','disputed') AND id != ?5)",
            params![self.agent_id.as_str(),input.memory.title,scope_type(&input.memory.scope),scope_key(&input.memory.scope),input.target_memory_id.as_deref().unwrap_or("")],|row|row.get(0),
        ).map_err(map_db)?;
        let similar = automatic
            && has_lexically_similar_memory(
                &transaction,
                &self.agent_id,
                &input.memory,
                input.target_memory_id.as_deref(),
            )?;
        let (outcome, reason) = if conflict {
            (
                AutoMemoryOutcome::PendingReview,
                "possible_semantic_conflict",
            )
        } else if similar {
            (
                AutoMemoryOutcome::PendingReview,
                "possible_semantic_duplicate_or_conflict",
            )
        } else {
            evaluate_auto_memory_evidence(&input.anchors, now)
        };
        receipt.disposition = if outcome == AutoMemoryOutcome::Activated {
            IntakeDisposition::Activated
        } else {
            IntakeDisposition::PendingReview
        };
        receipt.reason = reason.into();
        if let Some(mut current) = target {
            receipt.memory_id = Some(current.id.clone());
            if automatic || receipt.disposition == IntakeDisposition::PendingReview {
                receipt.disposition = IntakeDisposition::PendingReview;
                receipt.reason = "revision_requires_human_review".into();
                let id = format!("artirp_{}", ulid::Ulid::new());
                receipt.proposal_id = Some(id.clone());
                let proposal = MemoryRevisionProposal {
                    proposal_id: id.clone(),
                    target_memory_id: current.id.clone(),
                    expected_revision: current.current_revision,
                    artifact: input.memory.clone(),
                    anchors: input.anchors.clone(),
                    receipt: receipt.clone(),
                };
                transaction.execute("INSERT INTO memory_revision_proposals(proposal_id,agent_id,target_memory_id,expected_revision,status,proposal_json) VALUES (?1,?2,?3,?4,'pending',?5)",params![id,self.agent_id.as_str(),current.id,current.current_revision,encode(&proposal)?]).map_err(map_db)?;
            } else {
                current.title.clone_from(&input.memory.title);
                current.summary.clone_from(&input.memory.summary);
                current.revise(
                    input.memory.payload.clone(),
                    "user-requested intake revision",
                    now,
                )?;
                store_revision(&transaction, &self.agent_id, &current, &input.anchors)?;
                policy_decision(&transaction, &current, &input.anchors, &receipt)?;
            }
        } else {
            let mut stored = input.memory.clone();
            insert_memory_rows(&transaction, &self.agent_id, &stored, &input.anchors)?;
            if receipt.disposition == IntakeDisposition::Activated {
                stored.transition(MemoryStatus::Active, now)?;
                update_artifact(&transaction, &stored, None)?;
            }
            receipt.memory_id = Some(stored.id.clone());
            policy_decision(&transaction, &stored, &input.anchors, &receipt)?;
        }
        finish(transaction, &self.agent_id, &input, &receipt)
    }

    pub fn pending_revision_proposals(&self) -> ArtResult<Vec<MemoryRevisionProposal>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare("SELECT proposal_json FROM memory_revision_proposals WHERE agent_id=?1 AND status='pending' ORDER BY proposal_id").map_err(map_db)?;
        statement
            .query_map([self.agent_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(map_db)?
            .map(|row| decode(&row.map_err(map_db)?))
            .collect()
    }

    pub fn memory_revision_reviews(&self) -> ArtResult<Vec<serde_json::Value>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare("SELECT json_extract(proposal_json,'$.review') FROM memory_revision_proposals WHERE agent_id=?1 AND status IN ('confirmed','rejected') ORDER BY json_extract(proposal_json,'$.review.reviewed_at') DESC LIMIT 100").map_err(map_db)?;
        statement
            .query_map([self.agent_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(map_db)?
            .map(|row| decode(&row.map_err(map_db)?))
            .collect()
    }

    pub fn intake_receipts(&self) -> ArtResult<Vec<MemoryIntakeReceipt>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare("SELECT receipt_json FROM memory_intake_receipts WHERE agent_id=?1 ORDER BY received_at DESC").map_err(map_db)?;
        statement
            .query_map([self.agent_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(map_db)?
            .map(|row| decode(&row.map_err(map_db)?))
            .collect()
    }
}

fn has_lexically_similar_memory(
    transaction: &Transaction<'_>,
    agent_id: &AgentId,
    candidate: &MemoryArtifact,
    excluded_id: Option<&str>,
) -> ArtResult<bool> {
    let candidate_tokens = similarity_tokens(&format!("{} {}", candidate.title, candidate.summary));
    // Short records do not carry enough lexical signal to distinguish a
    // paraphrase from two intentionally distinct facts in the same family.
    if candidate_tokens.len() < 8 {
        return Ok(false);
    }
    let mut statement = transaction
        .prepare(
            "SELECT title,summary FROM memory_artifacts WHERE agent_id=?1 AND scope_type=?2 AND scope_key=?3 AND status IN ('active','candidate','disputed') AND id != ?4 ORDER BY updated_at DESC LIMIT 512",
        )
        .map_err(map_db)?;
    let existing = statement
        .query_map(
            params![
                agent_id.as_str(),
                scope_type(&candidate.scope),
                scope_key(&candidate.scope),
                excluded_id.unwrap_or("")
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(map_db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_db)?;
    Ok(existing.into_iter().any(|(title, summary)| {
        let tokens = similarity_tokens(&format!("{title} {summary}"));
        tokens.len() >= 8 && token_containment_at_least(&candidate_tokens, &tokens, 72)
    }))
}

fn similarity_tokens(value: &str) -> BTreeSet<String> {
    let normalized = value.nfkc().collect::<String>().to_lowercase();
    let mut tokens = BTreeSet::new();
    let mut latin = String::new();
    let mut non_latin_run = Vec::new();
    let flush_latin = |latin: &mut String, tokens: &mut BTreeSet<String>| {
        if latin.len() >= 2 {
            tokens.insert(std::mem::take(latin));
        } else {
            latin.clear();
        }
    };
    let flush_non_latin = |run: &mut Vec<char>, tokens: &mut BTreeSet<String>| {
        for pair in run.windows(2) {
            tokens.insert(pair.iter().collect());
        }
        run.clear();
    };
    for character in normalized.chars() {
        if character.is_ascii_alphanumeric() {
            flush_non_latin(&mut non_latin_run, &mut tokens);
            latin.push(character);
        } else if character.is_alphanumeric() {
            flush_latin(&mut latin, &mut tokens);
            non_latin_run.push(character);
        } else {
            flush_latin(&mut latin, &mut tokens);
            flush_non_latin(&mut non_latin_run, &mut tokens);
        }
    }
    flush_latin(&mut latin, &mut tokens);
    flush_non_latin(&mut non_latin_run, &mut tokens);
    tokens
}

fn token_containment_at_least(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    percent: usize,
) -> bool {
    let denominator = left.len().min(right.len());
    if denominator == 0 {
        return false;
    }
    left.intersection(right).count() * 100 >= denominator * percent
}

fn legacy_capture_matches(
    tx: &Transaction<'_>,
    agent: &AgentId,
    input: &MemoryIntakeRequest,
    receipt: &MemoryIntakeReceipt,
) -> ArtResult<bool> {
    if receipt.origin != IntakeOrigin::LegacyUnspecified {
        return Ok(false);
    }
    let historical_hash: Option<String> = tx.query_row(
        "SELECT payload_hash FROM capture_receipts WHERE agent_id=?1 AND idempotency_key=?2 AND id=?3",
        params![agent.as_str(),input.idempotency_key,receipt.receipt_id], |row|row.get(0),
    ).optional().map_err(map_db)?;
    let Some(hash) = historical_hash else {
        return Ok(false);
    };
    if hash != receipt.payload_hash {
        return Ok(false);
    }
    if let (Some(target), Some(expected)) = (&input.target_memory_id, input.expected_revision) {
        return Ok(receipt.memory_id.as_ref() == Some(target)
            && hash
                == super::revision_payload_hash(
                    target,
                    expected,
                    &input.memory.title,
                    &input.memory.summary,
                    &input.memory.payload,
                    &input.anchors,
                    "agent revision",
                ));
    }
    if input.target_memory_id.is_some() || input.expected_revision.is_some() {
        return Ok(false);
    }
    if hash == capture_payload_hash(&input.memory, &input.anchors) {
        return Ok(true);
    }
    if input.memory.status != MemoryStatus::Candidate {
        return Ok(false);
    }
    // The former MCP capture adapter activated sourced artifacts before passing
    // them to capture(). Only this historical state difference is normalized;
    // the original receipt bytes and all content/evidence fields stay exact.
    let mut formerly_active = input.memory.clone();
    formerly_active.status = MemoryStatus::Active;
    Ok(hash == capture_payload_hash(&formerly_active, &input.anchors))
}

fn validate_request(
    input: &MemoryIntakeRequest,
    origin: IntakeOrigin,
    agent: &AgentId,
) -> ArtResult<()> {
    if input.memory.agent_id != *agent || input.anchors.iter().any(|a| a.owner_agent_id != *agent) {
        return Err(ArtError::IdentityMismatch);
    }
    let bounded = |value: &str| !value.trim().is_empty() && value.len() <= 1024;
    if origin == IntakeOrigin::LegacyUnspecified
        || !bounded(&input.idempotency_key)
        || input.memory.status != MemoryStatus::Candidate
        || input.memory.current_revision != 1
        || input.memory.title.trim().is_empty()
        || input.memory.summary.trim().is_empty()
        || scope_key(&input.memory.scope).trim().is_empty()
        || input.target_memory_id.is_some() != input.expected_revision.is_some()
        || (origin != IntakeOrigin::HookTriggered && input.hook_trigger_receipt_id.is_some())
        || input
            .attribution
            .session_id
            .iter()
            .chain(input.attribution.turn_id.iter())
            .any(|s| !bounded(s))
        || (origin == IntakeOrigin::UserRequested
            && !input.request_basis.as_deref().is_some_and(bounded))
        || (origin == IntakeOrigin::AgentInitiated
            && !input.value_reason.as_deref().is_some_and(bounded))
        || input
            .request_basis
            .iter()
            .chain(input.value_reason.iter())
            .any(|s| !bounded(s))
    {
        return Err(ArtError::InvalidInput(
            "invalid memory intake origin, attribution, basis, value, or revision".into(),
        ));
    }
    input.memory.payload.validate()?;
    if input.anchors.is_empty() {
        return Err(ArtError::SourceRequired);
    }
    let hash = art_domain::memory::canonical_json_hash(
        &serde_json::to_value(&input.memory.payload)
            .map_err(|e| ArtError::Internal(e.to_string()))?,
    );
    if hash != input.memory.current_hash
        || input.memory.revisions.len() != 1
        || input.memory.revisions[0].revision != 1
        || input.memory.revisions[0].content_hash != hash
        || art_domain::memory::canonical_json_hash(
            &serde_json::to_value(&input.memory.revisions[0].payload)
                .map_err(|e| ArtError::Internal(e.to_string()))?,
        ) != hash
    {
        return Err(ArtError::InvalidInput(
            "memory payload and revision hash mismatch".into(),
        ));
    }
    for anchor in &input.anchors {
        let canonical = SourceAnchor::new_with_source(
            anchor.owner_agent_id.clone(),
            anchor.kind,
            anchor.locator.clone(),
            anchor.source_version.clone(),
            anchor.source_digest.clone(),
            anchor.excerpt.clone(),
            anchor.metadata.clone(),
            anchor.sensitivity,
            anchor.observed_at,
        )?;
        if canonical.content_hash != anchor.content_hash
            || canonical.excerpt_hash != anchor.excerpt_hash
        {
            return Err(ArtError::InvalidInput("source anchor hash mismatch".into()));
        }
    }
    Ok(())
}

fn sensitive_note(note: Option<&str>) -> ArtResult<bool> {
    let pattern = Regex::new(r"(?i)(authorization\s*:\s*bearer|begin\s+(rsa|openssh|ec)\s+private\s+key|api[_-]?key\s*[:=]|password\s*[:=]|cookie\s*[:=]|recovery\s+code|full[_ -]?transcript)").map_err(|e|ArtError::Internal(e.to_string()))?;
    Ok(note.is_some_and(|value| pattern.is_match(value)))
}

fn finish(
    transaction: Transaction<'_>,
    agent: &AgentId,
    input: &MemoryIntakeRequest,
    receipt: &MemoryIntakeReceipt,
) -> ArtResult<MemoryIntakeResult> {
    transaction.execute("INSERT INTO memory_intake_receipts(receipt_id,agent_id,idempotency_key,payload_hash,trigger_receipt_id,origin,disposition,memory_id,received_at,receipt_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![receipt.receipt_id,agent.as_str(),input.idempotency_key,receipt.payload_hash,input.hook_trigger_receipt_id,enum_name(&receipt.origin)?,enum_name(&receipt.disposition)?,receipt.memory_id,receipt.received_at,encode(receipt)?]).map_err(map_db)?;
    if receipt.origin == IntakeOrigin::HookTriggered
        && let Some(trigger) = &input.hook_trigger_receipt_id
        && let Some(outcome) = legacy_outcome(receipt.disposition)
    {
        insert_auto_submission_receipt(
            &transaction,
            agent.as_str(),
            trigger,
            0,
            &capture_payload_hash(&input.memory, &input.anchors),
            &AutoMemorySubmission {
                receipt_id: receipt.receipt_id.clone(),
                memory_id: receipt.memory_id.clone(),
                outcome,
                reason: receipt.reason.clone(),
                policy_version: receipt.policy_version.clone(),
                config_version: receipt.config_version,
                replayed: false,
            },
        )?;
    }
    transaction.commit().map_err(map_db)?;
    Ok(receipt.clone().result(false))
}

pub(super) fn legacy_outcome(disposition: IntakeDisposition) -> Option<AutoMemoryOutcome> {
    match disposition {
        IntakeDisposition::Activated => Some(AutoMemoryOutcome::Activated),
        IntakeDisposition::PendingReview => Some(AutoMemoryOutcome::PendingReview),
        IntakeDisposition::Duplicate => Some(AutoMemoryOutcome::Duplicate),
        IntakeDisposition::Rejected => Some(AutoMemoryOutcome::Rejected),
        _ => None,
    }
}

fn policy_decision(
    tx: &Transaction<'_>,
    memory: &MemoryArtifact,
    anchors: &[SourceAnchor],
    receipt: &MemoryIntakeReceipt,
) -> ArtResult<()> {
    let outcome = if receipt.disposition == IntakeDisposition::Activated {
        AssuranceOutcome::PartiallyCorroborated
    } else {
        AssuranceOutcome::NeedsReview
    };
    let decision = AssuranceDecision::new(
        &memory.id,
        memory.current_revision,
        outcome,
        anchor_set_hash(anchors),
        format!("policy:{}", receipt.policy_version),
        &receipt.reason,
        Utc::now(),
    )?;
    tx.execute("INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,?4,?5,'policy',?6,?7,?8)",params![decision.id,memory.id,memory.current_revision,format!("{outcome:?}").to_lowercase(),decision.anchor_set_hash,decision.actor,decision.rationale,decision.decided_at.to_rfc3339()]).map_err(map_db)?;
    Ok(())
}

fn store_revision(
    tx: &Transaction<'_>,
    agent: &AgentId,
    memory: &MemoryArtifact,
    anchors: &[SourceAnchor],
) -> ArtResult<()> {
    let revision = memory
        .revisions
        .last()
        .ok_or_else(|| ArtError::Internal("missing revision".into()))?;
    tx.execute("INSERT INTO memory_revisions(memory_id,revision,canonical_json,content_hash,changed_by,changed_at,change_reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![memory.id,revision.revision,encode(&revision.payload)?,revision.content_hash,agent.as_str(),revision.changed_at.to_rfc3339(),revision.reason]).map_err(map_db)?;
    for anchor in anchors {
        let existing: Option<String> = tx
            .query_row(
                "SELECT content_hash FROM source_anchors WHERE id=?1",
                [&anchor.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_db)?;
        if existing.is_some_and(|hash| hash != anchor.content_hash) {
            return Err(ArtError::DuplicateConflict);
        }
        tx.execute("INSERT OR IGNORE INTO source_anchors(id,owner_agent_id,kind,locator,source_version,source_digest,excerpt,excerpt_hash,sensitivity,observed_at,metadata_json,content_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![anchor.id,agent.as_str(),format!("{:?}",anchor.kind).to_lowercase(),anchor.locator,anchor.source_version,anchor.source_digest,anchor.excerpt,anchor.excerpt_hash,format!("{:?}",anchor.sensitivity).to_lowercase(),anchor.observed_at.to_rfc3339(),encode(&anchor.metadata)?,anchor.content_hash]).map_err(map_db)?;
        tx.execute("INSERT INTO memory_anchor_links(memory_id,memory_revision,anchor_id,role) VALUES (?1,?2,?3,'evidence')",params![memory.id,memory.current_revision,anchor.id]).map_err(map_db)?;
    }
    update_artifact(tx, memory, None)?;
    tx.execute("DELETE FROM memory_fts WHERE memory_id=?1", [&memory.id])
        .map_err(map_db)?;
    tx.execute(
        "INSERT INTO memory_fts(memory_id,revision,search_text) VALUES (?1,?2,?3)",
        params![memory.id, memory.current_revision, search_document(memory)],
    )
    .map_err(map_db)?;
    Ok(())
}

pub(super) fn linked_admission(
    tx: &Transaction<'_>,
    agent: &AgentId,
    attribution: &IntakeAttribution,
) -> ArtResult<Option<String>> {
    if !attribution.host_supplied
        || attribution.session_id.is_none()
        || attribution.turn_id.is_none()
    {
        return Ok(None);
    }
    tx.query_row("SELECT admission_id FROM memory_intake_budget WHERE agent_id=?1 AND session_id=?2 AND turn_id=?3",params![agent.as_str(),attribution.session_id,attribution.turn_id],|row|row.get(0)).optional().map_err(map_db)
}

fn admission_consumed(
    tx: &Transaction<'_>,
    agent: &AgentId,
    admission_id: &str,
) -> ArtResult<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_intake_receipts WHERE agent_id=?1 AND json_extract(receipt_json,'$.budget_association')=?2 AND disposition IN ('activated','pending_review'))",
        params![agent.as_str(),admission_id], |row|row.get(0),
    ).map_err(map_db)
}

fn unused_hook_admission(
    tx: &Transaction<'_>,
    agent: &AgentId,
    attribution: &IntakeAttribution,
    config_version: u64,
) -> ArtResult<Option<String>> {
    if !attribution.host_supplied
        || attribution.session_id.is_none()
        || attribution.turn_id.is_none()
    {
        return Ok(None);
    }
    // Only an unconsumed, current Hook claim can fund an ordinary submission.
    // A prior ordinary intake is never a reusable reservation; duplicates and
    // same-key retries have already returned above the admission boundary.
    let reservation: Option<String> = tx.query_row(
        "SELECT b.admission_id FROM memory_intake_budget b JOIN auto_memory_trigger_receipts t ON t.receipt_id=b.admission_id AND t.agent_id=b.agent_id WHERE b.agent_id=?1 AND b.session_id=?2 AND b.turn_id=?3 AND t.outcome='accepted' AND t.config_version=?4",
        params![agent.as_str(),attribution.session_id,attribution.turn_id,config_version],|row|row.get(0),
    ).optional().map_err(map_db)?;
    if let Some(id) = &reservation
        && admission_consumed(tx, agent, id)?
    {
        return Ok(None);
    }
    Ok(reservation)
}

pub(super) fn budget_limit(
    tx: &Transaction<'_>,
    agent: &AgentId,
    bucket: &str,
    status: &AutoMemoryStatus,
    now: DateTime<Utc>,
) -> ArtResult<Option<&'static str>> {
    let (count,last): (u32,Option<String>) = tx.query_row("SELECT COUNT(*),MAX(admitted_at) FROM memory_intake_budget WHERE agent_id=?1 AND bucket=?2",params![agent.as_str(),bucket],|row|Ok((row.get(0)?,row.get(1)?))).map_err(map_db)?;
    if count >= status.max_captures_per_session {
        return Ok(Some("session_limit"));
    }
    if last
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .is_some_and(|last| {
            now.signed_duration_since(last).num_seconds()
                < i64::try_from(status.cooldown_seconds).unwrap_or(i64::MAX)
        })
    {
        return Ok(Some("cooldown"));
    }
    Ok(None)
}

pub(super) fn admit(
    tx: &Transaction<'_>,
    agent: &AgentId,
    id: &str,
    bucket: &str,
    attribution: &IntakeAttribution,
    now: DateTime<Utc>,
) -> ArtResult<()> {
    tx.execute("INSERT INTO memory_intake_budget(admission_id,agent_id,bucket,session_id,turn_id,admitted_at) VALUES (?1,?2,?3,?4,?5,?6)",params![id,agent.as_str(),bucket,if attribution.host_supplied { attribution.session_id.as_deref() } else { None },if attribution.host_supplied { attribution.turn_id.as_deref() } else { None },now.to_rfc3339()]).map_err(map_db)?;
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> ArtResult<String> {
    serde_json::to_string(value).map_err(|e| ArtError::Internal(e.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> ArtResult<T> {
    serde_json::from_str(value).map_err(|e| ArtError::Internal(e.to_string()))
}
fn enum_name<T: Serialize>(value: &T) -> ArtResult<String> {
    decode(&encode(value)?)
}

pub(super) fn backup_before_migration(
    connection: &Connection,
    path: &Path,
    agent: &AgentId,
) -> ArtResult<()> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='art_meta')",
            [],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    if !exists {
        return Ok(());
    }
    let meta: Option<(String, String, i64)> = connection
        .query_row(
            "SELECT database_kind,agent_id,schema_version FROM art_meta LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(map_db)?;
    if let Some((kind, owner, version)) = meta {
        if kind != "agent-vault" || owner != agent.as_str() {
            return Err(ArtError::IdentityMismatch);
        }
        if version > SCHEMA_VERSION {
            return Err(ArtError::SchemaTooNew);
        }
        if version < SCHEMA_VERSION {
            let backup = path.with_extension(format!("pre-v4-{}.sqlite3", ulid::Ulid::new()));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)
                .map_err(|e| ArtError::Io(e.to_string()))?;
            set_private_permissions(&backup)?;
            drop(file);
            connection
                .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
                .map_err(map_db)?;
        }
    }
    Ok(())
}

pub(super) fn migrate_intake(tx: &Transaction<'_>, agent: &AgentId) -> ArtResult<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS memory_intake_receipts(receipt_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,payload_hash TEXT NOT NULL,trigger_receipt_id TEXT,origin TEXT NOT NULL,disposition TEXT NOT NULL,memory_id TEXT,received_at TEXT NOT NULL,receipt_json TEXT NOT NULL,UNIQUE(agent_id,idempotency_key),UNIQUE(agent_id,trigger_receipt_id));
        CREATE TABLE IF NOT EXISTS memory_intake_budget(admission_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,bucket TEXT NOT NULL,session_id TEXT,turn_id TEXT,admitted_at TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS memory_intake_budget_bucket ON memory_intake_budget(agent_id,bucket);
        CREATE TABLE IF NOT EXISTS memory_revision_proposals(proposal_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,target_memory_id TEXT NOT NULL REFERENCES memory_artifacts(id),expected_revision INTEGER NOT NULL,status TEXT NOT NULL,proposal_json TEXT NOT NULL);").map_err(map_db)?;
    let mut statement = tx.prepare("SELECT id,idempotency_key,payload_hash,accepted_memory_id,received_at FROM capture_receipts WHERE agent_id=?1 AND id NOT IN (SELECT receipt_id FROM memory_intake_receipts)").map_err(map_db)?;
    let rows = statement
        .query_map([agent.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(map_db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_db)?;
    for (id, key, hash, memory_id, time) in rows {
        let receipt = MemoryIntakeReceipt {
            schema: MEMORY_INTAKE_RECEIPT_SCHEMA.into(),
            receipt_id: id,
            origin: IntakeOrigin::LegacyUnspecified,
            attribution: IntakeAttribution::agent_asserted(None, None),
            payload_hash: hash,
            evidence_hash: String::new(),
            budget_bucket: String::new(),
            budget_association: None,
            disposition: IntakeDisposition::PendingReview,
            memory_id: Some(memory_id),
            proposal_id: None,
            reason: "legacy_unspecified".into(),
            request_basis: None,
            value_reason: None,
            policy_version: "legacy_unspecified".into(),
            config_version: 0,
            received_at: time,
        };
        tx.execute("INSERT OR IGNORE INTO memory_intake_receipts(receipt_id,agent_id,idempotency_key,payload_hash,origin,disposition,memory_id,received_at,receipt_json) VALUES (?1,?2,?3,?4,'legacy_unspecified','pending_review',?5,?6,?7)",params![receipt.receipt_id,agent.as_str(),key,receipt.payload_hash,receipt.memory_id,receipt.received_at,encode(&receipt)?]).map_err(map_db)?;
    }
    let mut statement = tx.prepare("SELECT receipt_id,trigger_receipt_id,payload_hash,memory_id,outcome,policy_version,reason,config_version,received_at FROM auto_memory_submission_receipts WHERE agent_id=?1 AND receipt_id NOT IN (SELECT receipt_id FROM memory_intake_receipts)").map_err(map_db)?;
    let rows = statement
        .query_map([agent.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, u64>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(map_db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_db)?;
    for (id, trigger, hash, memory_id, outcome, policy, reason, version, time) in rows {
        let payload_hash = art_domain::memory::canonical_json_hash(
            &serde_json::json!({"content":hash,"target_memory_id":null,"expected_revision":null}),
        );
        let receipt = MemoryIntakeReceipt {
            schema: MEMORY_INTAKE_RECEIPT_SCHEMA.into(),
            receipt_id: id,
            origin: IntakeOrigin::LegacyUnspecified,
            attribution: IntakeAttribution::agent_asserted(None, None),
            payload_hash,
            evidence_hash: String::new(),
            budget_bucket: String::new(),
            budget_association: Some(trigger.clone()),
            disposition: decode(&encode(&outcome)?)?,
            memory_id,
            proposal_id: None,
            reason,
            request_basis: None,
            value_reason: None,
            policy_version: policy,
            config_version: version,
            received_at: time,
        };
        tx.execute("INSERT OR IGNORE INTO memory_intake_receipts(receipt_id,agent_id,idempotency_key,payload_hash,trigger_receipt_id,origin,disposition,memory_id,received_at,receipt_json) VALUES (?1,?2,?3,?4,?5,'legacy_unspecified',?6,?7,?8,?9)",params![receipt.receipt_id,agent.as_str(),format!("hook:{trigger}:0"),receipt.payload_hash,trigger,outcome,receipt.memory_id,receipt.received_at,encode(&receipt)?]).map_err(map_db)?;
    }
    // Preserve historical accepted Hook claims in the shared budget.
    tx.execute("INSERT OR IGNORE INTO memory_intake_budget(admission_id,agent_id,bucket,session_id,turn_id,admitted_at) SELECT receipt_id,agent_id,'session:' || session_id,session_id,turn_id,created_at FROM auto_memory_trigger_receipts WHERE outcome='accepted' AND receipt_id NOT IN (SELECT trigger_receipt_id FROM memory_intake_receipts WHERE disposition='duplicate' AND trigger_receipt_id IS NOT NULL)",[]).map_err(map_db)?;
    Ok(())
}
