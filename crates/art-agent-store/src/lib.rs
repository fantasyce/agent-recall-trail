//! Physically isolated per-Agent SQLite Vaults.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use art_domain::{
    ArtError, ArtResult,
    agent::AgentId,
    anchor::{AnchorKind, AssuranceDecision, AssuranceOutcome, SourceAnchor, anchor_set_hash},
    memory::{MemoryArtifact, MemoryStatus, Sensitivity},
};
use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

mod intake;
pub use intake::*;

const SCHEMA_VERSION: i64 = 4;

const AUTO_MEMORY_CONFIG_SCHEMA: &str = "art.auto-memory.config.v1";
pub const AUTO_MEMORY_POLICY_VERSION: &str = "art.auto-memory.policy.v1";
pub const DEFAULT_AUTO_MEMORY_MAX_CAPTURES: u32 = 3;
pub const DEFAULT_AUTO_MEMORY_COOLDOWN_SECONDS: u64 = 600;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedAutoMemoryConfig {
    schema: String,
    enabled: bool,
    max_captures_per_session: u32,
    cooldown_seconds: u64,
    policy_version: String,
    config_version: u64,
    updated_at: String,
    updated_by: String,
    #[serde(default)]
    audit: Vec<AutoMemoryConfigAudit>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AutoMemoryConfigAudit {
    enabled: bool,
    config_version: u64,
    actor: String,
    changed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoMemoryStatus {
    pub enabled: bool,
    pub configured: bool,
    pub reason: Option<String>,
    pub max_captures_per_session: u32,
    pub cooldown_seconds: u64,
    pub policy_version: String,
    pub config_version: u64,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMemoryOutcome {
    Activated,
    PendingReview,
    Duplicate,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoMemorySubmission {
    pub receipt_id: String,
    pub memory_id: Option<String>,
    pub outcome: AutoMemoryOutcome,
    pub reason: String,
    pub policy_version: String,
    pub config_version: u64,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoMemoryTriggerClaim {
    pub receipt_id: String,
    pub accepted: bool,
    pub reason: String,
    pub replayed: bool,
    pub config_version: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoMemoryCandidateRecord {
    pub artifact: MemoryArtifact,
    pub anchors: Vec<SourceAnchor>,
    pub receipt_id: String,
    pub policy_version: String,
    pub policy_reason: String,
    pub received_at: String,
    pub origin: IntakeOrigin,
    pub value_reason: Option<String>,
    pub request_basis: Option<String>,
    pub attribution: IntakeAttribution,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoMemoryDiagnostics {
    pub pending_count: u64,
    pub last_hook_at: Option<String>,
    pub last_hook_outcome: Option<String>,
    pub last_capture_at: Option<String>,
    pub last_capture_outcome: Option<String>,
    pub origins: serde_json::Value,
    pub intake_summary: serde_json::Value,
    pub trigger_summary: serde_json::Value,
    pub shared_budget: Vec<serde_json::Value>,
    pub pending_revision_count: usize,
}

fn count_values(
    connection: &Connection,
    sql: &str,
    agent_id: &str,
    known_values: &[&str],
) -> ArtResult<serde_json::Value> {
    let mut counts = serde_json::Map::new();
    for value in known_values {
        counts.insert((*value).into(), serde_json::json!(0));
    }
    let mut statement = connection.prepare(sql).map_err(map_db)?;
    let rows = statement
        .query_map([agent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })
        .map_err(map_db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_db)?;
    for (value, count) in rows {
        counts.insert(value, serde_json::json!(count));
    }
    Ok(serde_json::Value::Object(counts))
}

impl AutoMemoryStatus {
    fn disabled(configured: bool, reason: &str) -> Self {
        Self {
            enabled: false,
            configured,
            reason: Some(reason.into()),
            max_captures_per_session: DEFAULT_AUTO_MEMORY_MAX_CAPTURES,
            cooldown_seconds: DEFAULT_AUTO_MEMORY_COOLDOWN_SECONDS,
            policy_version: AUTO_MEMORY_POLICY_VERSION.into(),
            config_version: 0,
            updated_at: None,
            updated_by: None,
        }
    }
}

/// Machine-wide, fail-closed automatic-memory configuration.
///
/// Mutation is intentionally not exposed by ART's MCP tool surface. The
/// authenticated governance UI owns the only user-facing update path.
#[derive(Debug, Clone)]
pub struct AutoMemoryConfigStore {
    path: PathBuf,
}

struct AutoMemoryConfigLock {
    path: PathBuf,
}

impl Drop for AutoMemoryConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl AutoMemoryConfigStore {
    pub fn new(art_root: impl AsRef<Path>) -> Self {
        Self {
            path: art_root.as_ref().join("config/art/auto-memory.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn mutation_guard(&self) -> ArtResult<AutoMemoryConfigLock> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| ArtError::InvalidInput("invalid automatic memory path".into()))?;
        fs::create_dir_all(parent).map_err(|error| ArtError::Io(error.to_string()))?;
        set_private_directory(parent)?;
        acquire_auto_memory_config_lock(parent)
    }

    pub fn status(&self) -> AutoMemoryStatus {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return AutoMemoryStatus::disabled(false, "configuration_missing");
            }
            Err(_) => return AutoMemoryStatus::disabled(true, "configuration_unreadable"),
        };
        let Ok(config) = serde_json::from_slice::<PersistedAutoMemoryConfig>(&bytes) else {
            return AutoMemoryStatus::disabled(true, "configuration_invalid");
        };
        if config.schema != AUTO_MEMORY_CONFIG_SCHEMA
            || config.policy_version != AUTO_MEMORY_POLICY_VERSION
            || config.config_version == 0
            || config.max_captures_per_session == 0
            || config.max_captures_per_session > 100
            || config.cooldown_seconds > 86_400
            || DateTime::parse_from_rfc3339(&config.updated_at).is_err()
            || config.updated_by.trim().is_empty()
        {
            return AutoMemoryStatus::disabled(true, "configuration_invalid");
        }
        AutoMemoryStatus {
            enabled: config.enabled,
            configured: true,
            reason: None,
            max_captures_per_session: config.max_captures_per_session,
            cooldown_seconds: config.cooldown_seconds,
            policy_version: config.policy_version,
            config_version: config.config_version,
            updated_at: Some(config.updated_at),
            updated_by: Some(config.updated_by),
        }
    }

    pub fn set_enabled(&self, enabled: bool, human_actor: &str) -> ArtResult<AutoMemoryStatus> {
        if human_actor.trim().is_empty() || !human_actor.starts_with("human:") {
            return Err(ArtError::PermissionDenied(
                "automatic memory settings require a human governance actor".into(),
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| ArtError::InvalidInput("invalid automatic memory path".into()))?;
        fs::create_dir_all(parent).map_err(|error| ArtError::Io(error.to_string()))?;
        set_private_directory(parent)?;
        let _lock = self.mutation_guard()?;

        let prior = self.status();
        let mut audit = fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedAutoMemoryConfig>(&bytes).ok())
            .map(|config| config.audit)
            .unwrap_or_default();
        let config_version = prior.config_version.saturating_add(1).max(1);
        let updated_at = Utc::now().to_rfc3339();
        audit.push(AutoMemoryConfigAudit {
            enabled,
            config_version,
            actor: human_actor.trim().into(),
            changed_at: updated_at.clone(),
        });
        if audit.len() > 100 {
            audit.drain(0..audit.len() - 100);
        }
        let config = PersistedAutoMemoryConfig {
            schema: AUTO_MEMORY_CONFIG_SCHEMA.into(),
            enabled,
            max_captures_per_session: prior.max_captures_per_session,
            cooldown_seconds: prior.cooldown_seconds,
            policy_version: AUTO_MEMORY_POLICY_VERSION.into(),
            config_version,
            updated_at,
            updated_by: human_actor.trim().into(),
            audit,
        };
        let bytes = serde_json::to_vec_pretty(&config)
            .map_err(|error| ArtError::Internal(error.to_string()))?;
        let temporary = parent.join(format!(".auto-memory.{}.tmp", ulid::Ulid::new()));
        let write_result = (|| -> ArtResult<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|error| ArtError::Io(error.to_string()))?;
            set_private_permissions(&temporary)?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| ArtError::Io(error.to_string()))?;
            fs::rename(&temporary, &self.path).map_err(|error| ArtError::Io(error.to_string()))?;
            set_private_permissions(&self.path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        let status = self.status();
        if !status.configured || status.config_version != config.config_version {
            return Err(ArtError::Internal(
                "automatic memory configuration did not persist".into(),
            ));
        }
        Ok(status)
    }
}

fn acquire_auto_memory_config_lock(parent: &Path) -> ArtResult<AutoMemoryConfigLock> {
    let path = parent.join(".auto-memory.lock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => {
                set_private_permissions(&path)?;
                file.sync_all()
                    .map_err(|error| ArtError::Io(error.to_string()))?;
                return Ok(AutoMemoryConfigLock { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
                    .is_ok_and(|age| age > std::time::Duration::from_secs(30));
                if stale {
                    let _ = fs::remove_file(&path);
                    continue;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(ArtError::DbBusy);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(ArtError::Io(error.to_string())),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentVaultDiagnostics {
    pub schema_version: i64,
    pub migration_checksum: String,
    pub database_kind: String,
    pub bound_agent_id: String,
    pub integrity_ok: bool,
    pub foreign_key_violations: u64,
    pub journal_mode: String,
    pub memory_count: u64,
    pub search_index_count: u64,
    pub search_index_aligned: bool,
    pub navigation_count: u64,
    pub navigation_aligned: bool,
    pub revision_count: u64,
    pub anchor_count: u64,
    pub wal_bytes: u64,
    pub file_mode: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RankedMemoryCandidate {
    pub artifact: MemoryArtifact,
    pub lexical_rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryNavigationEntry {
    pub memory_id: String,
    pub agent_id: AgentId,
    pub kind: String,
    pub scope_type: String,
    pub scope_key: String,
    pub title: String,
    pub status: String,
    pub revision: u32,
    pub updated_at: String,
    pub usage_count: u64,
    pub source_epoch: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryExportRecord {
    pub schema: String,
    pub artifact: MemoryArtifact,
    pub anchors: Vec<SourceAnchor>,
}

#[derive(Debug, Clone)]
pub struct AgentVault {
    path: PathBuf,
    agent_id: AgentId,
}

impl AgentVault {
    pub fn open(path: impl AsRef<Path>, agent_id: AgentId) -> ArtResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| ArtError::Io(error.to_string()))?;
            set_private_directory(parent)?;
        }
        let existed = path.exists();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match open_connection(&path).and_then(|mut connection| {
                intake::backup_before_migration(&connection, &path, &agent_id)?;
                migrate(&mut connection, &agent_id)?;
                Ok(())
            }) {
                Ok(()) => break,
                Err(ArtError::DbBusy) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
        if !existed {
            set_private_permissions(&path)?;
        }
        Ok(Self { path, agent_id })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub fn capture(
        &self,
        memory: &MemoryArtifact,
        anchors: &[SourceAnchor],
        idempotency_key: &str,
    ) -> ArtResult<MemoryArtifact> {
        if memory.agent_id != self.agent_id
            || anchors
                .iter()
                .any(|anchor| anchor.owner_agent_id != self.agent_id)
        {
            return Err(ArtError::IdentityMismatch);
        }
        if idempotency_key.trim().is_empty() {
            return Err(ArtError::InvalidInput("idempotency key is required".into()));
        }
        if anchors.is_empty() && memory.status != art_domain::memory::MemoryStatus::Candidate {
            return Err(ArtError::SourceRequired);
        }
        let payload_hash = capture_payload_hash(memory, anchors);
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT payload_hash, accepted_memory_id FROM capture_receipts WHERE agent_id = ?1 AND idempotency_key = ?2",
                params![self.agent_id.as_str(), idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(map_db)?;
        if let Some((existing_hash, memory_id)) = existing {
            if existing_hash != payload_hash {
                return Err(ArtError::DuplicateConflict);
            }
            transaction.rollback().map_err(map_db)?;
            return self.read(&memory_id);
        }

        insert_memory_rows(&transaction, &self.agent_id, memory, anchors)?;
        if memory.status == MemoryStatus::Active {
            let decision = AssuranceDecision::new(
                &memory.id,
                memory.current_revision,
                AssuranceOutcome::PartiallyCorroborated,
                anchor_set_hash(anchors),
                "policy:structured-source-admission",
                "typed payload and source anchors passed deterministic admission",
                Utc::now(),
            )?;
            transaction.execute(
                "INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,'partiallycorroborated',?4,'policy',?5,?6,?7)",
                params![decision.id,memory.id,memory.current_revision,decision.anchor_set_hash,decision.actor,decision.rationale,decision.decided_at.to_rfc3339()],
            ).map_err(map_db)?;
        }
        transaction.execute(
            "INSERT INTO capture_receipts (id, agent_id, idempotency_key, payload_hash, accepted_memory_id, received_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![format!("artr_{}", ulid::Ulid::new()), self.agent_id.as_str(), idempotency_key, payload_hash, memory.id, chrono::Utc::now().to_rfc3339()],
        ).map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(memory.clone())
    }

    pub fn submit_auto_candidate(
        &self,
        config: &AutoMemoryConfigStore,
        memory: &MemoryArtifact,
        anchors: &[SourceAnchor],
        trigger_receipt_id: &str,
        candidate_index: u32,
    ) -> ArtResult<AutoMemorySubmission> {
        if !config.status().enabled {
            return Err(ArtError::PermissionDenied(
                "global automatic memory is disabled".into(),
            ));
        }
        if candidate_index != 0 || trigger_receipt_id.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "automatic candidate requires trigger receipt and index 0".into(),
            ));
        }
        let result = self.intake(
            config,
            MemoryIntakeRequest {
                capture_origin: Some(IntakeOrigin::HookTriggered),
                memory: memory.clone(),
                anchors: anchors.to_vec(),
                idempotency_key: format!("hook:{trigger_receipt_id}:0"),
                attribution: IntakeAttribution::agent_asserted(None, None),
                request_basis: None,
                value_reason: None,
                target_memory_id: None,
                expected_revision: None,
                hook_trigger_receipt_id: Some(trigger_receipt_id.into()),
            },
        )?;
        let outcome = intake::legacy_outcome(result.disposition)
            .ok_or_else(|| ArtError::PermissionDenied(result.reason.clone()))?;
        Ok(AutoMemorySubmission {
            receipt_id: result.receipt_id,
            memory_id: result.memory_id,
            outcome,
            reason: result.reason,
            policy_version: result.policy_version,
            config_version: result.config_version,
            replayed: result.replayed,
        })
    }

    pub fn record_auto_correction_signal(
        &self,
        config: &AutoMemoryConfigStore,
        session_id: &str,
        turn_id: &str,
        signal: &str,
        now: DateTime<Utc>,
    ) -> ArtResult<bool> {
        let status = config.status();
        if !status.enabled {
            return Ok(false);
        }
        if session_id.trim().is_empty()
            || turn_id.trim().is_empty()
            || signal != "user_correction"
            || session_id.len() > 256
            || turn_id.len() > 256
        {
            return Err(ArtError::InvalidInput(
                "invalid bounded automatic-memory correction signal".into(),
            ));
        }
        let connection = open_connection(&self.path)?;
        let changed = connection.execute(
            "INSERT OR IGNORE INTO auto_memory_prompt_signals(agent_id,session_id,turn_id,signal,config_version,created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![self.agent_id.as_str(),session_id,turn_id,signal,status.config_version,now.to_rfc3339()],
        ).map_err(map_db)?;
        Ok(changed == 1)
    }

    pub fn record_auto_memory_hook_run(
        &self,
        config: &AutoMemoryConfigStore,
        outcome: &str,
        now: DateTime<Utc>,
    ) -> ArtResult<bool> {
        let status = config.status();
        if !status.enabled {
            return Ok(false);
        }
        if !matches!(
            outcome,
            "not_admitted"
                | "correction_signal"
                | "accepted"
                | "cooldown"
                | "session_limit"
                | "turn_already_handled"
                | "global_disabled"
        ) {
            return Err(ArtError::InvalidInput(
                "invalid automatic-memory hook outcome".into(),
            ));
        }
        let connection = open_connection(&self.path)?;
        connection.execute(
            "INSERT INTO auto_memory_hook_runs(agent_id,last_run_at,last_outcome,config_version) VALUES (?1,?2,?3,?4) ON CONFLICT(agent_id) DO UPDATE SET last_run_at=excluded.last_run_at,last_outcome=excluded.last_outcome,config_version=excluded.config_version",
            params![self.agent_id.as_str(),now.to_rfc3339(),outcome,status.config_version],
        ).map_err(map_db)?;
        Ok(true)
    }

    pub fn has_auto_correction_signal(&self, session_id: &str, turn_id: &str) -> ArtResult<bool> {
        let connection = open_connection(&self.path)?;
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM auto_memory_prompt_signals WHERE agent_id=?1 AND session_id=?2 AND turn_id=?3 AND signal='user_correction')",
            params![self.agent_id.as_str(),session_id,turn_id],
            |row| row.get(0),
        ).map_err(map_db)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_auto_memory_trigger(
        &self,
        config: &AutoMemoryConfigStore,
        session_id: &str,
        turn_id: &str,
        event_hash: &str,
        now: DateTime<Utc>,
    ) -> ArtResult<AutoMemoryTriggerClaim> {
        self.claim_auto_memory_trigger_with_attribution(
            config,
            &IntakeAttribution::host_supplied(session_id.into(), Some(turn_id.into())),
            event_hash,
            now,
        )
    }

    /// Host adapters must mark identity trusted only when supplied by the host.
    pub fn claim_auto_memory_trigger_with_attribution(
        &self,
        config: &AutoMemoryConfigStore,
        attribution: &IntakeAttribution,
        event_hash: &str,
        now: DateTime<Utc>,
    ) -> ArtResult<AutoMemoryTriggerClaim> {
        let status = config.status();
        if !status.enabled {
            return Ok(AutoMemoryTriggerClaim {
                receipt_id: String::new(),
                accepted: false,
                reason: "global_disabled".into(),
                replayed: false,
                config_version: status.config_version,
            });
        }
        if attribution
            .session_id
            .iter()
            .chain(attribution.turn_id.iter())
            .any(|s| s.trim().is_empty() || s.len() > 256)
            || event_hash.len() != 64
            || !event_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ArtError::InvalidInput(
                "invalid automatic-memory trigger identity".into(),
            ));
        }
        let bucket = attribution.bucket(now);
        // Missing identity is keyed by the bounded event digest for Hook replay,
        // never promoted to a trusted session or turn in the admission records.
        let session_id = if attribution.host_supplied {
            attribution.session_id.as_deref()
        } else {
            None
        }
        .unwrap_or(&bucket);
        let turn_id = if attribution.host_supplied {
            attribution.turn_id.as_deref()
        } else {
            None
        }
        .unwrap_or(event_hash);
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let existing: Option<(String, String, String, u64)> = transaction.query_row(
            "SELECT receipt_id,event_hash,outcome,config_version FROM auto_memory_trigger_receipts WHERE agent_id=?1 AND session_id=?2 AND turn_id=?3",
            params![self.agent_id.as_str(),session_id,turn_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).optional().map_err(map_db)?;
        if let Some((receipt_id, existing_hash, outcome, config_version)) = existing {
            if existing_hash != event_hash {
                return Err(ArtError::DuplicateConflict);
            }
            return Ok(AutoMemoryTriggerClaim {
                receipt_id,
                accepted: outcome == "accepted",
                reason: outcome,
                replayed: true,
                config_version,
            });
        }
        let _guard = config.mutation_guard()?;
        let status = config.status();
        let outcome = if !status.enabled {
            "global_disabled"
        } else if intake::linked_admission(&transaction, &self.agent_id, attribution)?.is_some() {
            "turn_already_handled"
        } else {
            intake::budget_limit(&transaction, &self.agent_id, &bucket, &status, now)?
                .unwrap_or("accepted")
        };
        let receipt_id = format!("artamt_{}", ulid::Ulid::new());
        if outcome == "accepted" {
            intake::admit(
                &transaction,
                &self.agent_id,
                &receipt_id,
                &bucket,
                attribution,
                now,
            )?;
        }
        transaction.execute(
            "INSERT INTO auto_memory_trigger_receipts(receipt_id,agent_id,session_id,turn_id,event_hash,outcome,config_version,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![receipt_id,self.agent_id.as_str(),session_id,turn_id,event_hash,outcome,status.config_version,now.to_rfc3339()],
        ).map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(AutoMemoryTriggerClaim {
            receipt_id,
            accepted: outcome == "accepted",
            reason: outcome.into(),
            replayed: false,
            config_version: status.config_version,
        })
    }

    pub fn auto_memory_candidates(&self) -> ArtResult<Vec<AutoMemoryCandidateRecord>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare(
            "SELECT r.receipt_id,r.memory_id,json_extract(r.receipt_json,'$.policy_version'),json_extract(r.receipt_json,'$.reason'),r.received_at,r.receipt_json FROM memory_intake_receipts r JOIN memory_artifacts m ON m.id=r.memory_id WHERE r.agent_id=?1 AND r.disposition='pending_review' AND json_extract(r.receipt_json,'$.proposal_id') IS NULL AND m.status='candidate' ORDER BY r.received_at DESC LIMIT 100",
        ).map_err(map_db)?;
        let rows = statement
            .query_map([self.agent_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(map_db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_db)?;
        drop(statement);
        rows.into_iter()
            .map(
                |(
                    receipt_id,
                    memory_id,
                    policy_version,
                    policy_reason,
                    received_at,
                    receipt_json,
                )| {
                    let receipt: MemoryIntakeReceipt = serde_json::from_str(&receipt_json)
                        .map_err(|error| ArtError::Internal(error.to_string()))?;
                    let exported = self.export_record(&memory_id)?;
                    Ok(AutoMemoryCandidateRecord {
                        artifact: exported.artifact,
                        anchors: exported.anchors,
                        receipt_id,
                        policy_version,
                        policy_reason,
                        received_at,
                        origin: receipt.origin,
                        value_reason: receipt.value_reason,
                        request_basis: receipt.request_basis,
                        attribution: receipt.attribution,
                    })
                },
            )
            .collect()
    }

    pub fn auto_memory_diagnostics(&self) -> ArtResult<AutoMemoryDiagnostics> {
        let connection = open_connection(&self.path)?;
        let pending_count: u64 = connection.query_row(
            "SELECT COUNT(DISTINCT m.id) FROM memory_intake_receipts r JOIN memory_artifacts m ON m.id=r.memory_id WHERE r.agent_id=?1 AND r.disposition='pending_review' AND json_extract(r.receipt_json,'$.proposal_id') IS NULL AND m.status='candidate'",
            [self.agent_id.as_str()], |row| row.get(0),
        ).map_err(map_db)?;
        let last_hook: Option<(String, String)> = connection
            .query_row(
                "SELECT last_run_at,last_outcome FROM auto_memory_hook_runs WHERE agent_id=?1",
                [self.agent_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(map_db)?;
        let last_capture: Option<(String,String)> = connection.query_row(
            "SELECT received_at,disposition FROM memory_intake_receipts WHERE agent_id=?1 ORDER BY received_at DESC LIMIT 1",
            [self.agent_id.as_str()], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(map_db)?;
        let receipts = self.intake_receipts()?;
        let mut origins = serde_json::json!({});
        for origin in [
            IntakeOrigin::AgentInitiated,
            IntakeOrigin::HookTriggered,
            IntakeOrigin::UserRequested,
            IntakeOrigin::LegacyUnspecified,
        ] {
            let name =
                serde_json::to_value(origin).map_err(|e| ArtError::Internal(e.to_string()))?;
            let entries: Vec<_> = receipts.iter().filter(|r| r.origin == origin).collect();
            origins[name.as_str().unwrap_or("legacy_unspecified")] = serde_json::json!({"count":entries.len(),"last_disposition":entries.first().map(|r| r.disposition),"recent":entries.into_iter().take(10).collect::<Vec<_>>()});
        }
        let mut statement = connection.prepare("SELECT bucket,COUNT(*),MAX(admitted_at) FROM memory_intake_budget WHERE agent_id=?1 GROUP BY bucket ORDER BY MAX(admitted_at) DESC LIMIT 100").map_err(map_db)?;
        let shared_budget = statement.query_map([self.agent_id.as_str()], |row| {
            let bucket: String = row.get(0)?;
            Ok(serde_json::json!({"degraded":bucket.starts_with("agent-day:"),"bucket":bucket,"used":row.get::<_,u64>(1)?,"last_admitted_at":row.get::<_,String>(2)?}))
        }).map_err(map_db)?.collect::<Result<Vec<_>,_>>().map_err(map_db)?;
        let intake_summary = count_values(
            &connection,
            "SELECT disposition,COUNT(*) FROM memory_intake_receipts WHERE agent_id=?1 GROUP BY disposition",
            self.agent_id.as_str(),
            &[
                "activated",
                "pending_review",
                "duplicate",
                "rejected",
                "disabled",
                "rate_limited",
            ],
        )?;
        let trigger_summary = count_values(
            &connection,
            "SELECT outcome,COUNT(*) FROM auto_memory_trigger_receipts WHERE agent_id=?1 GROUP BY outcome",
            self.agent_id.as_str(),
            &[
                "accepted",
                "cooldown",
                "session_limit",
                "disabled",
                "duplicate_event",
            ],
        )?;
        let pending_revision_count = self.pending_revision_proposals()?.len();
        Ok(AutoMemoryDiagnostics {
            pending_count: pending_count + pending_revision_count as u64,
            last_hook_at: last_hook.as_ref().map(|value| value.0.clone()),
            last_hook_outcome: last_hook.map(|value| value.1),
            last_capture_at: last_capture.as_ref().map(|value| value.0.clone()),
            last_capture_outcome: last_capture.map(|value| value.1),
            origins,
            intake_summary,
            trigger_summary,
            shared_budget,
            pending_revision_count,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_and_confirm_auto_candidate(
        &self,
        memory_id: &str,
        expected_revision: u32,
        title: &str,
        summary: &str,
        payload: art_domain::memory::MemoryPayload,
        actor: &str,
        reason: &str,
    ) -> ArtResult<MemoryArtifact> {
        if !actor.starts_with("human:")
            || title.trim().is_empty()
            || summary.trim().is_empty()
            || reason.trim().is_empty()
            || reason.trim().len() > 1_000
        {
            return Err(ArtError::InvalidInput(
                "human candidate edit requires bounded title, summary, actor, and reason".into(),
            ));
        }
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let is_auto: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_intake_receipts WHERE agent_id=?1 AND memory_id=?2 AND disposition='pending_review' AND json_extract(receipt_json,'$.proposal_id') IS NULL)",
            params![self.agent_id.as_str(),memory_id], |row| row.get(0),
        ).map_err(map_db)?;
        let mut memory = read_in_transaction(&transaction, &self.agent_id, memory_id)?;
        if !is_auto
            || memory.status != MemoryStatus::Candidate
            || memory.current_revision != expected_revision
        {
            return Err(ArtError::SourceStale);
        }
        let anchor_ids: Vec<String> = {
            let mut statement = transaction.prepare(
                "SELECT anchor_id FROM memory_anchor_links WHERE memory_id=?1 AND memory_revision=?2 ORDER BY anchor_id",
            ).map_err(map_db)?;
            statement
                .query_map(params![memory_id, expected_revision], |row| row.get(0))
                .map_err(map_db)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_db)?
        };
        if anchor_ids.is_empty() {
            return Err(ArtError::SourceRequired);
        }
        title.trim().clone_into(&mut memory.title);
        summary.trim().clone_into(&mut memory.summary);
        memory.revise(payload, reason.trim(), Utc::now())?;
        if auto_memory_contains_sensitive(&memory, &[])? {
            return Err(ArtError::InvalidInput(
                "sensitive candidate edit rejected".into(),
            ));
        }
        let revision = memory
            .revisions
            .last()
            .ok_or_else(|| ArtError::Internal("revision missing".into()))?;
        transaction.execute(
            "INSERT INTO memory_revisions(memory_id,revision,canonical_json,content_hash,changed_by,changed_at,change_reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![memory.id,revision.revision,serde_json::to_string(&revision.payload).map_err(|error| ArtError::Internal(error.to_string()))?,revision.content_hash,actor,revision.changed_at.to_rfc3339(),revision.reason],
        ).map_err(map_db)?;
        for anchor_id in &anchor_ids {
            transaction.execute(
                "INSERT INTO memory_anchor_links(memory_id,memory_revision,anchor_id,role) VALUES (?1,?2,?3,'evidence')",
                params![memory.id,memory.current_revision,anchor_id],
            ).map_err(map_db)?;
        }
        memory.transition(MemoryStatus::Active, Utc::now())?;
        update_artifact(&transaction, &memory, None)?;
        transaction
            .execute("DELETE FROM memory_fts WHERE memory_id=?1", [&memory.id])
            .map_err(map_db)?;
        transaction
            .execute(
                "INSERT INTO memory_fts(memory_id,revision,search_text) VALUES (?1,?2,?3)",
                params![memory.id, memory.current_revision, search_document(&memory)],
            )
            .map_err(map_db)?;
        let anchor_hash =
            anchor_set_hash_in_transaction(&transaction, &memory.id, memory.current_revision)?;
        let decision = AssuranceDecision::new(
            &memory.id,
            memory.current_revision,
            AssuranceOutcome::Corroborated,
            anchor_hash,
            actor,
            reason.trim(),
            Utc::now(),
        )?;
        transaction.execute(
            "INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,'corroborated',?4,'human',?5,?6,?7)",
            params![decision.id,memory.id,memory.current_revision,decision.anchor_set_hash,actor,reason.trim(),decision.decided_at.to_rfc3339()],
        ).map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(memory)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn revise(
        &self,
        memory_id: &str,
        expected_revision: u32,
        title: &str,
        summary: &str,
        payload: art_domain::memory::MemoryPayload,
        anchors: &[SourceAnchor],
        reason: &str,
        idempotency_key: &str,
    ) -> ArtResult<MemoryArtifact> {
        if title.trim().is_empty()
            || summary.trim().is_empty()
            || reason.trim().is_empty()
            || idempotency_key.trim().is_empty()
        {
            return Err(ArtError::InvalidInput(
                "revision title, summary, reason, and idempotency key are required".into(),
            ));
        }
        if anchors.is_empty()
            || anchors
                .iter()
                .any(|anchor| anchor.owner_agent_id != self.agent_id)
        {
            return Err(ArtError::SourceRequired);
        }
        let payload_hash = revision_payload_hash(
            memory_id,
            expected_revision,
            title,
            summary,
            &payload,
            anchors,
            reason,
        );
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT payload_hash,accepted_memory_id FROM capture_receipts WHERE agent_id=?1 AND idempotency_key=?2",
                params![self.agent_id.as_str(), idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(map_db)?;
        if let Some((existing_hash, existing_id)) = existing {
            if existing_hash != payload_hash || existing_id != memory_id {
                return Err(ArtError::DuplicateConflict);
            }
            transaction.rollback().map_err(map_db)?;
            return self.read(memory_id);
        }
        let mut memory = read_in_transaction(&transaction, &self.agent_id, memory_id)?;
        if memory.current_revision != expected_revision {
            return Err(ArtError::SourceStale);
        }
        title.clone_into(&mut memory.title);
        summary.clone_into(&mut memory.summary);
        memory.revise(payload, reason, Utc::now())?;
        let revision = memory
            .revisions
            .last()
            .ok_or_else(|| ArtError::Internal("revision was not created".into()))?;
        transaction
            .execute(
                "INSERT INTO memory_revisions(memory_id,revision,canonical_json,content_hash,changed_by,changed_at,change_reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![memory_id,revision.revision,serde_json::to_string(&revision.payload).map_err(|error| ArtError::Internal(error.to_string()))?,revision.content_hash,self.agent_id.as_str(),revision.changed_at.to_rfc3339(),revision.reason],
            )
            .map_err(map_db)?;
        for anchor in anchors {
            transaction.execute(
                "INSERT INTO source_anchors (id, owner_agent_id, kind, locator, source_version, source_digest, excerpt, excerpt_hash, sensitivity, observed_at, metadata_json, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![anchor.id, self.agent_id.as_str(), format!("{:?}", anchor.kind).to_lowercase(), anchor.locator, anchor.source_version, anchor.source_digest, anchor.excerpt, anchor.excerpt_hash, format!("{:?}", anchor.sensitivity).to_lowercase(), anchor.observed_at.to_rfc3339(), serde_json::to_string(&anchor.metadata).map_err(|error| ArtError::Internal(error.to_string()))?, anchor.content_hash],
            ).map_err(map_db)?;
            transaction
                .execute(
                    "INSERT INTO memory_anchor_links(memory_id,memory_revision,anchor_id,role) VALUES (?1,?2,?3,'evidence')",
                    params![memory_id, memory.current_revision, anchor.id],
                )
                .map_err(map_db)?;
        }
        let decision = AssuranceDecision::new(
            memory_id,
            memory.current_revision,
            AssuranceOutcome::PartiallyCorroborated,
            anchor_set_hash(anchors),
            "policy:structured-source-admission",
            "the new typed revision and source anchors passed deterministic admission",
            Utc::now(),
        )?;
        transaction.execute(
            "INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,'partiallycorroborated',?4,'policy',?5,?6,?7)",
            params![decision.id,memory_id,memory.current_revision,decision.anchor_set_hash,decision.actor,decision.rationale,decision.decided_at.to_rfc3339()],
        ).map_err(map_db)?;
        update_artifact(&transaction, &memory, None)?;
        transaction
            .execute("DELETE FROM memory_fts WHERE memory_id=?1", [memory_id])
            .map_err(map_db)?;
        transaction
            .execute(
                "INSERT INTO memory_fts(memory_id,revision,search_text) VALUES (?1,?2,?3)",
                params![memory_id, memory.current_revision, search_document(&memory)],
            )
            .map_err(map_db)?;
        transaction
            .execute(
                "INSERT INTO capture_receipts(id,agent_id,idempotency_key,payload_hash,accepted_memory_id,received_at) VALUES (?1,?2,?3,?4,?5,?6)",
                params![format!("artr_{}",ulid::Ulid::new()),self.agent_id.as_str(),idempotency_key,payload_hash,memory_id,Utc::now().to_rfc3339()],
            )
            .map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(memory)
    }

    pub fn read(&self, memory_id: &str) -> ArtResult<MemoryArtifact> {
        let connection = open_connection(&self.path)?;
        let value: Option<String> = connection
            .query_row(
                "SELECT artifact_json FROM memory_artifacts WHERE id = ?1 AND agent_id = ?2",
                params![memory_id, self.agent_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_db)?;
        value
            .map(|json| {
                serde_json::from_str(&json).map_err(|error| ArtError::Internal(error.to_string()))
            })
            .transpose()?
            .ok_or(ArtError::NotFound)
    }

    pub fn read_source_revision(
        &self,
        memory_id: &str,
        revision: u32,
    ) -> ArtResult<(MemoryArtifact, String)> {
        let memory = self.read(memory_id)?;
        if memory.current_revision != revision {
            return Err(ArtError::SourceStale);
        }
        let connection = open_connection(&self.path)?;
        let invalidated: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM source_events e JOIN memory_anchor_links l ON l.anchor_id=e.anchor_id WHERE l.memory_id=?1 AND l.memory_revision=?2",
                params![memory_id, revision],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if invalidated > 0 {
            return Err(ArtError::SourceStale);
        }
        let mut statement = connection
            .prepare(
                "SELECT a.id,a.content_hash FROM source_anchors a JOIN memory_anchor_links l ON l.anchor_id=a.id WHERE l.memory_id=?1 AND l.memory_revision=?2 ORDER BY a.id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(params![memory_id, revision], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_db)?;
        let anchors: Vec<_> = rows.collect::<Result<_, _>>().map_err(map_db)?;
        if anchors.is_empty() {
            return Err(ArtError::SourceRequired);
        }
        let anchor_hash = hex_digest(
            anchors
                .iter()
                .map(|(id, hash)| format!("{id}:{hash}"))
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes(),
        );
        Ok((memory, anchor_hash))
    }

    pub fn list(&self) -> ArtResult<Vec<MemoryArtifact>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare("SELECT artifact_json FROM memory_artifacts WHERE agent_id = ?1 ORDER BY updated_at DESC").map_err(map_db)?;
        let rows = statement
            .query_map([self.agent_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(map_db)?;
        rows.map(|row| {
            let json = row.map_err(map_db)?;
            serde_json::from_str(&json).map_err(|error| ArtError::Internal(error.to_string()))
        })
        .collect()
    }

    pub fn count(&self) -> ArtResult<u64> {
        let connection = open_connection(&self.path)?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM memory_artifacts WHERE agent_id = ?1",
                [self.agent_id.as_str()],
                |row| row.get(0),
            )
            .map_err(map_db)
    }

    pub fn index_epoch(&self) -> ArtResult<String> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection
            .prepare(
                "SELECT id,current_hash,status,updated_at FROM memory_artifacts WHERE agent_id=?1 ORDER BY id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map([self.agent_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(map_db)?;
        let mut hasher = Sha256::new();
        for row in rows {
            let (id, hash, status, updated) = row.map_err(map_db)?;
            hasher.update(id);
            hasher.update(hash);
            hasher.update(status);
            hasher.update(updated);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    pub fn semantic_index_epoch(&self, now: DateTime<Utc>) -> ArtResult<String> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection
            .prepare(
                "SELECT id,current_hash,status,updated_at,valid_from,valid_until FROM memory_artifacts WHERE agent_id=?1 AND status='active' AND (valid_from IS NULL OR valid_from<=?2) AND (valid_until IS NULL OR valid_until>?2) ORDER BY id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(params![self.agent_id.as_str(), now.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(map_db)?;
        let mut hasher = Sha256::new();
        for row in rows {
            let (id, hash, status, updated, valid_from, valid_until) = row.map_err(map_db)?;
            for value in [
                Some(id),
                Some(hash),
                Some(status),
                Some(updated),
                valid_from,
                valid_until,
            ] {
                hasher.update(value.unwrap_or_default());
                hasher.update([0]);
            }
        }
        Ok(hex::encode(hasher.finalize()))
    }

    pub fn rebuild_navigation(&self) -> ArtResult<u64> {
        let source_epoch = self.index_epoch()?;
        let eligible: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|memory| memory.status == MemoryStatus::Active)
            .collect();
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        transaction
            .execute("DELETE FROM memory_navigation", [])
            .map_err(map_db)?;
        for memory in &eligible {
            transaction.execute(
                "INSERT INTO memory_navigation(memory_id,agent_id,kind,scope_type,scope_key,title,status,revision,updated_at,usage_count,source_epoch) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,COALESCE((SELECT COUNT(*) FROM feedback_events WHERE subject_type='memory' AND subject_id=?1),0),?10)",
                params![
                    memory.id,
                    self.agent_id.as_str(),
                    memory.payload.kind_name(),
                    scope_type(&memory.scope),
                    scope_key(&memory.scope),
                    memory.title,
                    format!("{:?}", memory.status).to_lowercase(),
                    memory.current_revision,
                    memory.updated_at.to_rfc3339(),
                    source_epoch,
                ],
            ).map_err(map_db)?;
        }
        transaction
            .execute(
                "INSERT INTO memory_navigation_meta(singleton,source_epoch) VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET source_epoch=excluded.source_epoch",
                [&source_epoch],
            )
            .map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        u64::try_from(eligible.len()).map_err(|error| ArtError::Internal(error.to_string()))
    }

    pub fn navigation_entries(&self) -> ArtResult<Vec<MemoryNavigationEntry>> {
        let connection = open_connection(&self.path)?;
        let mut statement = connection.prepare(
            "SELECT memory_id,agent_id,kind,scope_type,scope_key,title,status,revision,updated_at,usage_count,source_epoch FROM memory_navigation WHERE agent_id=?1 ORDER BY scope_key,title,memory_id",
        ).map_err(map_db)?;
        let rows = statement
            .query_map([self.agent_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, u32>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, u64>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })
            .map_err(map_db)?;
        rows.map(|row| {
            let (
                memory_id,
                agent_id,
                kind,
                scope_type,
                scope_key,
                title,
                status,
                revision,
                updated_at,
                usage_count,
                source_epoch,
            ) = row.map_err(map_db)?;
            Ok(MemoryNavigationEntry {
                memory_id,
                agent_id: agent_id.parse()?,
                kind,
                scope_type,
                scope_key,
                title,
                status,
                revision,
                updated_at,
                usage_count,
                source_epoch,
            })
        })
        .collect()
    }

    pub fn navigation_aligned(&self) -> ArtResult<bool> {
        let connection = open_connection(&self.path)?;
        let projected: Option<String> = connection
            .query_row(
                "SELECT source_epoch FROM memory_navigation_meta WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_db)?;
        let canonical = self.index_epoch()?;
        Ok(projected.as_deref() == Some(canonical.as_str()))
    }

    pub fn search_ranked_candidates(
        &self,
        terms: &[String],
        limit: usize,
    ) -> ArtResult<Vec<RankedMemoryCandidate>> {
        if !(1..=2_048).contains(&limit) {
            return Err(ArtError::InvalidInput(
                "candidate limit must be 1..=2048".into(),
            ));
        }
        let connection = open_connection(&self.path)?;
        let expression = fts_expression(terms)?;
        let mut statement = connection
            .prepare(
                "SELECT a.artifact_json FROM memory_fts f JOIN memory_artifacts a ON a.id=f.memory_id WHERE memory_fts MATCH ?1 AND a.agent_id=?2 ORDER BY rank,a.updated_at DESC,a.id ASC LIMIT ?3",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(params![expression, self.agent_id.as_str(), limit], |row| {
                row.get::<_, String>(0)
            })
            .map_err(map_db)?;
        rows.enumerate()
            .map(|(index, row)| {
                let artifact = serde_json::from_str(&row.map_err(map_db)?)
                    .map_err(|error| ArtError::Internal(error.to_string()))?;
                Ok(RankedMemoryCandidate {
                    artifact,
                    lexical_rank: index + 1,
                })
            })
            .collect()
    }

    pub fn search_ranked_eligible_candidates(
        &self,
        terms: &[String],
        limit: usize,
        include_candidates: bool,
        now: DateTime<Utc>,
    ) -> ArtResult<Vec<RankedMemoryCandidate>> {
        if !(1..=2_048).contains(&limit) {
            return Err(ArtError::InvalidInput(
                "candidate limit must be 1..=2048".into(),
            ));
        }
        let connection = open_connection(&self.path)?;
        let expression = fts_expression(terms)?;
        let mut statement = connection
            .prepare(
                "SELECT a.artifact_json FROM memory_fts f JOIN memory_artifacts a ON a.id=f.memory_id WHERE memory_fts MATCH ?1 AND a.agent_id=?2 AND (a.status='active' OR (?3 AND a.status='candidate')) AND (a.valid_from IS NULL OR a.valid_from<=?4) AND (a.valid_until IS NULL OR a.valid_until>?4) ORDER BY rank,a.updated_at DESC,a.id ASC LIMIT ?5",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                params![
                    expression,
                    self.agent_id.as_str(),
                    include_candidates,
                    now.to_rfc3339(),
                    limit
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(map_db)?;
        rows.enumerate()
            .map(|(index, row)| {
                let artifact = serde_json::from_str(&row.map_err(map_db)?)
                    .map_err(|error| ArtError::Internal(error.to_string()))?;
                Ok(RankedMemoryCandidate {
                    artifact,
                    lexical_rank: index + 1,
                })
            })
            .collect()
    }

    pub fn search_candidates(&self, terms: &[String]) -> ArtResult<Vec<MemoryArtifact>> {
        self.search_ranked_candidates(terms, 512).map(|ranked| {
            ranked
                .into_iter()
                .map(|candidate| candidate.artifact)
                .collect()
        })
    }

    pub fn search_ranked_disputed_ids(
        &self,
        terms: &[String],
        limit: usize,
    ) -> ArtResult<Vec<String>> {
        if !(1..=2_048).contains(&limit) {
            return Err(ArtError::InvalidInput(
                "candidate limit must be 1..=2048".into(),
            ));
        }
        let connection = open_connection(&self.path)?;
        let expression = fts_expression(terms)?;
        let mut statement = connection
            .prepare(
                "SELECT a.id FROM memory_fts f JOIN memory_artifacts a ON a.id=f.memory_id WHERE memory_fts MATCH ?1 AND a.agent_id=?2 AND a.status='disputed' ORDER BY rank,a.updated_at DESC,a.id ASC LIMIT ?3",
            )
            .map_err(map_db)?;
        statement
            .query_map(params![expression, self.agent_id.as_str(), limit], |row| {
                row.get::<_, String>(0)
            })
            .map_err(map_db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_db)
    }

    pub fn rebuild_search_index(&self) -> ArtResult<u64> {
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        transaction
            .execute("DELETE FROM memory_fts", [])
            .map_err(map_db)?;
        let mut statement = transaction
            .prepare(
                "SELECT id,current_revision,artifact_json FROM memory_artifacts WHERE agent_id=?1 ORDER BY id",
            )
            .map_err(map_db)?;
        let rows: Vec<(String, u32, String)> = statement
            .query_map([self.agent_id.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(map_db)?
            .collect::<Result<_, _>>()
            .map_err(map_db)?;
        drop(statement);
        for (id, revision, artifact_json) in &rows {
            let memory: MemoryArtifact = serde_json::from_str(artifact_json)
                .map_err(|error| ArtError::Internal(error.to_string()))?;
            transaction
                .execute(
                    "INSERT INTO memory_fts(memory_id,revision,search_text) VALUES (?1,?2,?3)",
                    params![id, revision, search_document(&memory)],
                )
                .map_err(map_db)?;
        }
        transaction.commit().map_err(map_db)?;
        u64::try_from(rows.len()).map_err(|error| ArtError::Internal(error.to_string()))
    }

    pub fn append_feedback(
        &self,
        subject_type: &str,
        subject_id: &str,
        signal: &str,
        safe_note: Option<&str>,
        idempotency_key: &str,
    ) -> ArtResult<String> {
        if !matches!(signal, "relevant" | "stale" | "conflict" | "unsafe") {
            return Err(ArtError::InvalidInput("invalid feedback signal".into()));
        }
        if safe_note.is_some_and(|note| note.len() > 1_024) {
            return Err(ArtError::InvalidInput("feedback note is too large".into()));
        }
        if idempotency_key.trim().is_empty() {
            return Err(ArtError::InvalidInput("idempotency key is required".into()));
        }
        let payload_hash = hex_digest(
            serde_json::to_vec(&serde_json::json!({
                "subject_type":subject_type,
                "subject_id":subject_id,
                "signal":signal,
                "safe_note":safe_note
            }))
            .map_err(|error| ArtError::Internal(error.to_string()))?
            .as_slice(),
        );
        let id = format!("artf_{}", ulid::Ulid::new());
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT payload_hash,feedback_id FROM feedback_receipts WHERE agent_id=?1 AND idempotency_key=?2",
                params![self.agent_id.as_str(), idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(map_db)?;
        if let Some((existing_hash, existing_id)) = existing {
            if existing_hash != payload_hash {
                return Err(ArtError::DuplicateConflict);
            }
            return Ok(existing_id);
        }
        transaction
            .execute(
                "INSERT INTO feedback_events(id,agent_id,subject_type,subject_id,signal,safe_note,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![id, self.agent_id.as_str(), subject_type, subject_id, signal, safe_note, chrono::Utc::now().to_rfc3339()],
            )
            .map_err(map_db)?;
        transaction
            .execute(
                "INSERT INTO feedback_receipts(agent_id,idempotency_key,payload_hash,feedback_id,received_at) VALUES (?1,?2,?3,?4,?5)",
                params![self.agent_id.as_str(), idempotency_key, payload_hash, id, Utc::now().to_rfc3339()],
            )
            .map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(id)
    }

    pub fn assure(
        &self,
        memory_id: &str,
        revision: u32,
        outcome: AssuranceOutcome,
        actor: &str,
        reason: &str,
    ) -> ArtResult<AssuranceDecision> {
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let mut memory = read_in_transaction(&transaction, &self.agent_id, memory_id)?;
        if memory.current_revision != revision {
            return Err(ArtError::SourceStale);
        }
        let mut statement = transaction
            .prepare(
                "SELECT a.id,a.content_hash FROM source_anchors a JOIN memory_anchor_links l ON l.anchor_id=a.id WHERE l.memory_id=?1 AND l.memory_revision=?2 ORDER BY a.id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(params![memory_id, revision], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_db)?;
        let anchors: Vec<_> = rows.collect::<Result<_, _>>().map_err(map_db)?;
        drop(statement);
        if anchors.is_empty() {
            return Err(ArtError::SourceRequired);
        }
        let anchor_hash = hex_digest(
            anchors
                .iter()
                .map(|(id, hash)| format!("{id}:{hash}"))
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes(),
        );
        let decision = AssuranceDecision::new(
            memory_id,
            revision,
            outcome,
            anchor_hash,
            actor,
            reason,
            Utc::now(),
        )?;
        let target = match outcome {
            AssuranceOutcome::Corroborated | AssuranceOutcome::PartiallyCorroborated => {
                Some(MemoryStatus::Active)
            }
            AssuranceOutcome::Disputed | AssuranceOutcome::Invalidated => {
                Some(if memory.status == MemoryStatus::Candidate {
                    MemoryStatus::Rejected
                } else {
                    MemoryStatus::Disputed
                })
            }
            AssuranceOutcome::NeedsReview => None,
        };
        if let Some(target) = target
            && memory.status != target
        {
            memory.transition(target, Utc::now())?;
            update_artifact(&transaction, &memory, None)?;
        }
        transaction.execute(
            "INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,?4,?5,'human',?6,?7,?8)",
            params![decision.id, memory_id, revision, format!("{outcome:?}").to_lowercase(), decision.anchor_set_hash, actor, reason, decision.decided_at.to_rfc3339()],
        ).map_err(map_db)?;
        transaction.commit().map_err(map_db)?;
        Ok(decision)
    }

    pub fn record_source_change(
        &self,
        anchor_id: &str,
        new_digest: Option<&str>,
        revoked: bool,
        actor: &str,
        reason: &str,
    ) -> ArtResult<u64> {
        if actor.trim().is_empty() || reason.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "source change requires actor and reason".into(),
            ));
        }
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let owner: Option<String> = transaction
            .query_row(
                "SELECT owner_agent_id FROM source_anchors WHERE id=?1",
                [anchor_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_db)?;
        if owner.as_deref() != Some(self.agent_id.as_str()) {
            return Err(ArtError::NotFound);
        }
        let event_id = format!("artse_{}", ulid::Ulid::new());
        transaction.execute(
            "INSERT INTO source_events(id,anchor_id,event_type,new_digest,actor,reason,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![event_id,anchor_id,if revoked{"revoked"}else{"digest_changed"},new_digest,actor,reason,Utc::now().to_rfc3339()],
        ).map_err(map_db)?;
        let mut links = transaction
            .prepare(
                "SELECT memory_id,memory_revision FROM memory_anchor_links WHERE anchor_id=?1 ORDER BY memory_id",
            )
            .map_err(map_db)?;
        let linked: Vec<(String, u32)> = links
            .query_map([anchor_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(map_db)?
            .collect::<Result<_, _>>()
            .map_err(map_db)?;
        drop(links);
        let mut affected = 0_u64;
        for (memory_id, revision) in linked {
            let mut memory = read_in_transaction(&transaction, &self.agent_id, &memory_id)?;
            if memory.current_revision != revision {
                continue;
            }
            let anchor_hash = anchor_set_hash_in_transaction(&transaction, &memory_id, revision)?;
            let outcome = if revoked {
                AssuranceOutcome::Invalidated
            } else {
                AssuranceOutcome::NeedsReview
            };
            let decision = AssuranceDecision::new(
                &memory_id,
                revision,
                outcome,
                anchor_hash,
                actor,
                reason,
                Utc::now(),
            )?;
            transaction.execute(
                "INSERT INTO assurance_decisions(id,memory_id,memory_revision,outcome,anchor_set_hash,actor_kind,actor_id,rationale,decided_at) VALUES (?1,?2,?3,?4,?5,'human',?6,?7,?8)",
                params![decision.id,memory_id,revision,format!("{outcome:?}").to_lowercase(),decision.anchor_set_hash,actor,reason,decision.decided_at.to_rfc3339()],
            ).map_err(map_db)?;
            let target = if memory.status == MemoryStatus::Candidate {
                MemoryStatus::Rejected
            } else {
                MemoryStatus::Disputed
            };
            if memory.status != target {
                memory.transition(target, Utc::now())?;
                update_artifact(&transaction, &memory, None)?;
            }
            affected += 1;
        }
        transaction.commit().map_err(map_db)?;
        Ok(affected)
    }

    pub fn dispute(&self, memory_id: &str, reason: &str) -> ArtResult<()> {
        self.transition(memory_id, MemoryStatus::Disputed, reason, None)
    }

    pub fn supersede(&self, memory_id: &str, by: &str, reason: &str) -> ArtResult<()> {
        if memory_id == by {
            return Err(ArtError::InvalidInput(
                "a memory cannot supersede itself".into(),
            ));
        }
        self.read(by)?;
        self.transition(memory_id, MemoryStatus::Superseded, reason, Some(by))?;
        let connection = open_connection(&self.path)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO memory_relations(from_memory_id,relation_type,to_memory_id,created_at) VALUES (?1,'supersedes',?2,?3)",
                params![by, memory_id, Utc::now().to_rfc3339()],
            )
            .map_err(map_db)?;
        Ok(())
    }

    pub fn archive(&self, memory_id: &str, reason: &str) -> ArtResult<()> {
        self.transition(memory_id, MemoryStatus::Archived, reason, None)
    }

    fn transition(
        &self,
        memory_id: &str,
        target: MemoryStatus,
        reason: &str,
        superseded_by: Option<&str>,
    ) -> ArtResult<()> {
        if reason.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "lifecycle reason is required".into(),
            ));
        }
        let mut connection = open_connection(&self.path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let mut memory = read_in_transaction(&transaction, &self.agent_id, memory_id)?;
        memory.transition(target, Utc::now())?;
        if let Some(replacement) = superseded_by {
            memory.superseded_by = Some(replacement.to_owned());
        }
        update_artifact(&transaction, &memory, superseded_by)?;
        transaction
            .execute(
                "INSERT INTO lifecycle_events(id,memory_id,event_type,reason,actor,created_at) VALUES (?1,?2,?3,?4,'human:local-user',?5)",
                params![format!("arte_{}", ulid::Ulid::new()), memory_id, format!("{target:?}").to_lowercase(), reason, Utc::now().to_rfc3339()],
            )
            .map_err(map_db)?;
        transaction.commit().map_err(map_db)
    }

    #[doc(hidden)]
    pub fn test_only_set_schema_version(&self, version: i64) -> ArtResult<()> {
        let connection = open_connection(&self.path)?;
        connection
            .execute("UPDATE art_meta SET schema_version = ?1", [version])
            .map_err(map_db)?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn test_only_simulate_disk_full(&self) -> ArtResult<()> {
        let connection = open_connection(&self.path)?;
        connection
            .execute_batch(
                "CREATE TRIGGER art_test_disk_full BEFORE INSERT ON memory_artifacts BEGIN SELECT RAISE(FAIL,'database or disk is full'); END;",
            )
            .map_err(map_db)
    }

    pub fn checkpoint_wal(&self) -> ArtResult<()> {
        open_connection(&self.path)?
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(map_db)
    }

    pub fn integrity_check(&self) -> ArtResult<bool> {
        let connection = open_connection(&self.path)?;
        let result: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(map_db)?;
        Ok(result == "ok")
    }

    pub fn diagnostics(&self) -> ArtResult<AgentVaultDiagnostics> {
        let connection = open_connection(&self.path)?;
        let (database_kind, bound_agent_id, schema_version) = connection
            .query_row(
                "SELECT database_kind,agent_id,schema_version FROM art_meta LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(map_db)?;
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(map_db)?;
        let mut foreign_keys = connection
            .prepare("PRAGMA foreign_key_check")
            .map_err(map_db)?;
        let foreign_key_violations = foreign_keys
            .query_map([], |_| Ok(()))
            .map_err(map_db)?
            .count() as u64;
        drop(foreign_keys);
        let journal_mode = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(map_db)?;
        let count = |table: &str| -> ArtResult<u64> {
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .map_err(map_db)
        };
        let memory_count = count("memory_artifacts")?;
        let search_index_count = count("memory_fts")?;
        Ok(AgentVaultDiagnostics {
            schema_version,
            migration_checksum: hex_digest(b"art.agent-vault.schema.v3"),
            database_kind,
            bound_agent_id,
            integrity_ok: integrity == "ok",
            foreign_key_violations,
            journal_mode,
            memory_count,
            search_index_count,
            search_index_aligned: memory_count == search_index_count,
            navigation_count: count("memory_navigation")?,
            navigation_aligned: self.navigation_aligned()?,
            revision_count: count("memory_revisions")?,
            anchor_count: count("source_anchors")?,
            wal_bytes: fs::metadata(self.path.with_extension("sqlite3-wal"))
                .map_or(0, |metadata| metadata.len()),
            file_mode: file_mode(&self.path)?,
        })
    }

    pub fn export_record(&self, memory_id: &str) -> ArtResult<MemoryExportRecord> {
        let artifact = self.read(memory_id)?;
        let connection = open_connection(&self.path)?;
        let mut statement = connection
            .prepare(
                "SELECT a.id,a.kind,a.locator,a.source_version,a.source_digest,a.excerpt,a.excerpt_hash,a.sensitivity,a.observed_at,a.verified_at,a.metadata_json,a.content_hash FROM source_anchors a JOIN memory_anchor_links l ON l.anchor_id=a.id WHERE l.memory_id=?1 AND l.memory_revision=?2 ORDER BY a.id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(params![memory_id, artifact.current_revision], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                ))
            })
            .map_err(map_db)?;
        let mut anchors = Vec::new();
        for row in rows {
            let (
                id,
                kind,
                locator,
                source_version,
                source_digest,
                excerpt,
                excerpt_hash,
                sensitivity,
                observed_at,
                verified_at,
                metadata,
                content_hash,
            ) = row.map_err(map_db)?;
            anchors.push(SourceAnchor {
                id,
                owner_agent_id: self.agent_id.clone(),
                kind: parse_anchor_kind(&kind)?,
                locator,
                source_version,
                source_digest,
                excerpt,
                excerpt_hash,
                metadata: serde_json::from_str(&metadata)
                    .map_err(|error| ArtError::Internal(error.to_string()))?,
                sensitivity: parse_sensitivity(&sensitivity)?,
                observed_at: chrono::DateTime::parse_from_rfc3339(&observed_at)
                    .map_err(|error| ArtError::Internal(error.to_string()))?
                    .with_timezone(&Utc),
                verified_at: verified_at
                    .map(|value| {
                        chrono::DateTime::parse_from_rfc3339(&value)
                            .map(|date| date.with_timezone(&Utc))
                    })
                    .transpose()
                    .map_err(|error| ArtError::Internal(error.to_string()))?,
                content_hash,
            });
        }
        Ok(MemoryExportRecord {
            schema: "art.memory.export.v1".into(),
            artifact,
            anchors,
        })
    }

    pub fn import_record(&self, record: &MemoryExportRecord) -> ArtResult<MemoryArtifact> {
        if record.schema != "art.memory.export.v1"
            || record.artifact.agent_id != self.agent_id
            || record
                .anchors
                .iter()
                .any(|anchor| anchor.owner_agent_id != self.agent_id)
        {
            return Err(ArtError::IdentityMismatch);
        }
        record.artifact.payload.validate()?;
        let revision = record
            .artifact
            .revisions
            .last()
            .ok_or_else(|| ArtError::InvalidInput("memory revision is required".into()))?;
        if revision.revision != record.artifact.current_revision
            || revision.content_hash != record.artifact.current_hash
            || revision.content_hash
                != art_domain::memory::canonical_json_hash(
                    &serde_json::to_value(&record.artifact.payload)
                        .map_err(|error| ArtError::Internal(error.to_string()))?,
                )
        {
            return Err(ArtError::InvalidInput(
                "memory export hash or revision mismatch".into(),
            ));
        }
        for anchor in &record.anchors {
            let verified = SourceAnchor::new_with_source(
                self.agent_id.clone(),
                anchor.kind,
                anchor.locator.clone(),
                anchor.source_version.clone(),
                anchor.source_digest.clone(),
                anchor.excerpt.clone(),
                anchor.metadata.clone(),
                anchor.sensitivity,
                anchor.observed_at,
            )?;
            if verified.content_hash != anchor.content_hash
                || verified.excerpt_hash != anchor.excerpt_hash
            {
                return Err(ArtError::InvalidInput("source anchor hash mismatch".into()));
            }
        }
        self.capture(
            &record.artifact,
            &record.anchors,
            &format!(
                "import:{}:{}",
                record.artifact.id, record.artifact.current_revision
            ),
        )
    }
}

fn open_connection(path: &Path) -> ArtResult<Connection> {
    let connection = Connection::open(path).map_err(map_db)?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;").map_err(map_db)?;
    Ok(connection)
}

fn read_in_transaction(
    transaction: &Transaction<'_>,
    agent_id: &AgentId,
    memory_id: &str,
) -> ArtResult<MemoryArtifact> {
    let value: Option<String> = transaction
        .query_row(
            "SELECT artifact_json FROM memory_artifacts WHERE id=?1 AND agent_id=?2",
            params![memory_id, agent_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_db)?;
    value
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| ArtError::Internal(error.to_string()))
        })
        .transpose()?
        .ok_or(ArtError::NotFound)
}

fn anchor_set_hash_in_transaction(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision: u32,
) -> ArtResult<String> {
    let mut statement = transaction
        .prepare(
            "SELECT a.id,a.content_hash FROM source_anchors a JOIN memory_anchor_links l ON l.anchor_id=a.id WHERE l.memory_id=?1 AND l.memory_revision=?2 ORDER BY a.id",
        )
        .map_err(map_db)?;
    let rows = statement
        .query_map(params![memory_id, revision], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(map_db)?;
    let anchors: Vec<_> = rows.collect::<Result<_, _>>().map_err(map_db)?;
    if anchors.is_empty() {
        return Err(ArtError::SourceRequired);
    }
    Ok(hex_digest(
        anchors
            .iter()
            .map(|(id, hash)| format!("{id}:{hash}"))
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    ))
}

fn update_artifact(
    transaction: &Transaction<'_>,
    memory: &MemoryArtifact,
    superseded_by: Option<&str>,
) -> ArtResult<()> {
    let status = memory.status;
    transaction
        .execute(
            "UPDATE memory_artifacts SET status=?2,artifact_json=?3,updated_at=?4,superseded_by=COALESCE(?5,superseded_by),title=?6,summary=?7,current_revision=?8,current_hash=?9,valid_from=?10,valid_until=?11,review_after=?12 WHERE id=?1",
            params![
                memory.id,
                format!("{status:?}").to_lowercase(),
                serde_json::to_string(memory)
                    .map_err(|error| ArtError::Internal(error.to_string()))?,
                memory.updated_at.to_rfc3339(),
                superseded_by,
                memory.title,
                memory.summary,
                memory.current_revision,
                memory.current_hash,
                memory.valid_from.map(|value| value.to_rfc3339()),
                memory.valid_until.map(|value| value.to_rfc3339()),
                memory.review_after.map(|value| value.to_rfc3339()),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn insert_memory_rows(
    transaction: &Transaction<'_>,
    agent_id: &AgentId,
    memory: &MemoryArtifact,
    anchors: &[SourceAnchor],
) -> ArtResult<()> {
    let payload_json =
        serde_json::to_string(memory).map_err(|error| ArtError::Internal(error.to_string()))?;
    transaction
        .execute(
            "INSERT INTO memory_artifacts (id, agent_id, kind, status, title, summary, scope_type, scope_key, sensitivity, valid_from, valid_until, review_after, current_revision, current_hash, artifact_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![memory.id, agent_id.as_str(), memory.payload.kind_name(), format!("{:?}", memory.status).to_lowercase(), memory.title, memory.summary, scope_type(&memory.scope), scope_key(&memory.scope), format!("{:?}", memory.sensitivity).to_lowercase(), memory.valid_from.map(|value| value.to_rfc3339()), memory.valid_until.map(|value| value.to_rfc3339()), memory.review_after.map(|value| value.to_rfc3339()), memory.current_revision, memory.current_hash, payload_json, memory.created_at.to_rfc3339(), memory.updated_at.to_rfc3339()],
        )
        .map_err(map_db)?;
    transaction
        .execute(
            "INSERT INTO memory_fts(memory_id,revision,search_text) VALUES (?1,?2,?3)",
            params![memory.id, memory.current_revision, search_document(memory)],
        )
        .map_err(map_db)?;
    for revision in &memory.revisions {
        transaction.execute(
            "INSERT INTO memory_revisions (memory_id, revision, canonical_json, content_hash, changed_by, changed_at, change_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![memory.id, revision.revision, serde_json::to_string(&revision.payload).map_err(|error| ArtError::Internal(error.to_string()))?, revision.content_hash, agent_id.as_str(), revision.changed_at.to_rfc3339(), revision.reason],
        ).map_err(map_db)?;
    }
    for anchor in anchors {
        transaction.execute(
            "INSERT INTO source_anchors (id, owner_agent_id, kind, locator, source_version, source_digest, excerpt, excerpt_hash, sensitivity, observed_at, metadata_json, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![anchor.id, agent_id.as_str(), format!("{:?}", anchor.kind).to_lowercase(), anchor.locator, anchor.source_version, anchor.source_digest, anchor.excerpt, anchor.excerpt_hash, format!("{:?}", anchor.sensitivity).to_lowercase(), anchor.observed_at.to_rfc3339(), serde_json::to_string(&anchor.metadata).map_err(|error| ArtError::Internal(error.to_string()))?, anchor.content_hash],
        ).map_err(map_db)?;
        transaction.execute(
            "INSERT INTO memory_anchor_links (memory_id, memory_revision, anchor_id, role) VALUES (?1, ?2, ?3, 'evidence')",
            params![memory.id, memory.current_revision, anchor.id],
        ).map_err(map_db)?;
    }
    Ok(())
}

fn insert_auto_submission_receipt(
    transaction: &Transaction<'_>,
    agent_id: &str,
    trigger_receipt_id: &str,
    candidate_index: u32,
    payload_hash: &str,
    result: &AutoMemorySubmission,
) -> ArtResult<()> {
    transaction.execute(
        "INSERT INTO auto_memory_submission_receipts(receipt_id,agent_id,trigger_receipt_id,candidate_index,payload_hash,memory_id,outcome,policy_version,reason,config_version,received_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![result.receipt_id,agent_id,trigger_receipt_id,candidate_index,payload_hash,result.memory_id,auto_outcome_name(result.outcome),result.policy_version,result.reason,result.config_version,Utc::now().to_rfc3339()],
    ).map_err(map_db)?;
    Ok(())
}

const fn auto_outcome_name(outcome: AutoMemoryOutcome) -> &'static str {
    match outcome {
        AutoMemoryOutcome::Activated => "activated",
        AutoMemoryOutcome::PendingReview => "pending_review",
        AutoMemoryOutcome::Duplicate => "duplicate",
        AutoMemoryOutcome::Rejected => "rejected",
    }
}

fn auto_memory_contains_sensitive(
    memory: &MemoryArtifact,
    anchors: &[SourceAnchor],
) -> ArtResult<bool> {
    let pattern = Regex::new(
        r"(?i)(authorization\s*:\s*bearer|begin\s+(rsa|openssh|ec)\s+private\s+key|api[_-]?key\s*[:=]|password\s*[:=]|cookie\s*[:=]|recovery\s+code|full[_ -]?transcript)",
    )
    .map_err(|error| ArtError::Internal(error.to_string()))?;
    let memory_json = serde_json::to_string(&serde_json::json!({
        "title":memory.title,
        "summary":memory.summary,
        "payload":memory.payload,
    }))
    .map_err(|error| ArtError::Internal(error.to_string()))?;
    if pattern.is_match(&memory_json) {
        return Ok(true);
    }
    Ok(anchors.iter().any(|anchor| {
        anchor
            .excerpt
            .as_deref()
            .is_some_and(|excerpt| pattern.is_match(excerpt))
            || pattern.is_match(&anchor.locator)
            || sensitive_metadata(&anchor.metadata, &pattern)
    }))
}

fn sensitive_metadata(value: &serde_json::Value, pattern: &Regex) -> bool {
    match value {
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "password"
                    | "api_key"
                    | "apikey"
                    | "authorization"
                    | "cookie"
                    | "token"
                    | "private_key"
                    | "recovery_code"
                    | "full_transcript"
            ) || sensitive_metadata(value, pattern)
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| sensitive_metadata(value, pattern)),
        serde_json::Value::String(value) => pattern.is_match(value),
        _ => false,
    }
}

fn evaluate_auto_memory_evidence(
    anchors: &[SourceAnchor],
    now: DateTime<Utc>,
) -> (AutoMemoryOutcome, &'static str) {
    let recent = anchors.iter().all(|anchor| {
        anchor.observed_at <= now + chrono::Duration::minutes(5)
            && anchor.observed_at >= now - chrono::Duration::days(30)
    });
    if !recent {
        return (AutoMemoryOutcome::PendingReview, "source_not_current");
    }
    let mut verified = false;
    for anchor in anchors {
        let current = match anchor.kind {
            AnchorKind::FileSnapshot => verify_file_snapshot(anchor),
            AnchorKind::GitObject => verify_git_object(anchor),
            AnchorKind::CommandReceipt | AnchorKind::TestReceipt => {
                verify_execution_receipt(anchor)
            }
            _ => false,
        };
        if matches!(
            anchor.kind,
            AnchorKind::FileSnapshot | AnchorKind::GitObject
        ) && !current
        {
            return (AutoMemoryOutcome::PendingReview, "source_not_current");
        }
        verified |= current;
    }
    if verified {
        (AutoMemoryOutcome::Activated, "verified_scoped_receipt")
    } else {
        (
            AutoMemoryOutcome::PendingReview,
            "evidence_requires_human_review",
        )
    }
}

fn verify_execution_receipt(anchor: &SourceAnchor) -> bool {
    let path = Path::new(&anchor.locator);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 1024 * 1024
    {
        return false;
    }
    let Some(expected_digest) = anchor.source_digest.as_deref() else {
        return false;
    };
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    if !matches_sha256_digest(&bytes, expected_digest) {
        return false;
    }
    let Ok(receipt) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let evidence_scope = anchor
        .metadata
        .get("evidence_scope")
        .and_then(serde_json::Value::as_str);
    anchor
        .metadata
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
        == Some(0)
        && receipt.get("exit_code").and_then(serde_json::Value::as_i64) == Some(0)
        && evidence_scope.is_some_and(|value| !value.trim().is_empty() && value.len() <= 512)
        && receipt
            .get("evidence_scope")
            .and_then(serde_json::Value::as_str)
            == evidence_scope
}

fn verify_file_snapshot(anchor: &SourceAnchor) -> bool {
    let path = Path::new(&anchor.locator);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 16 * 1024 * 1024
    {
        return false;
    }
    let Some(expected) = anchor.source_digest.as_deref() else {
        return false;
    };
    fs::read(path).is_ok_and(|bytes| matches_sha256_digest(&bytes, expected))
}

fn matches_sha256_digest(bytes: &[u8], expected: &str) -> bool {
    // Accept the standard algorithm label at verification only. Keep the
    // original anchor and receipt hashes unchanged for exact replay.
    let digest = expected.strip_prefix("sha256:").unwrap_or(expected);
    hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(digest)
}

fn verify_git_object(anchor: &SourceAnchor) -> bool {
    let Some(repository) = anchor
        .metadata
        .get("repository_path")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let repository = Path::new(repository);
    let Some(expected) = anchor.source_version.as_deref() else {
        return false;
    };
    if !repository.is_absolute() || !repository.join(".git").exists() {
        return false;
    }
    Command::new("git")
        .args([
            "-C",
            repository.to_string_lossy().as_ref(),
            "rev-parse",
            "--verify",
            &format!("{}^{{object}}", anchor.locator),
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|actual| actual.trim() == expected)
}

fn migrate(connection: &mut Connection, agent_id: &AgentId) -> ArtResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS art_meta (database_kind TEXT NOT NULL, agent_id TEXT NOT NULL, schema_version INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS memory_artifacts (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL, scope_type TEXT NOT NULL, scope_key TEXT NOT NULL, sensitivity TEXT NOT NULL, valid_from TEXT, valid_until TEXT, review_after TEXT, current_revision INTEGER NOT NULL CHECK(current_revision >= 1), current_hash TEXT NOT NULL, artifact_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, superseded_by TEXT REFERENCES memory_artifacts(id));
         CREATE TABLE IF NOT EXISTS memory_revisions (memory_id TEXT NOT NULL REFERENCES memory_artifacts(id), revision INTEGER NOT NULL, canonical_json TEXT NOT NULL, content_hash TEXT NOT NULL, changed_by TEXT NOT NULL, changed_at TEXT NOT NULL, change_reason TEXT NOT NULL, PRIMARY KEY(memory_id, revision));
         CREATE TABLE IF NOT EXISTS source_anchors (id TEXT PRIMARY KEY, owner_agent_id TEXT NOT NULL, kind TEXT NOT NULL, locator TEXT NOT NULL, source_version TEXT, source_digest TEXT, excerpt TEXT, excerpt_hash TEXT, sensitivity TEXT NOT NULL, observed_at TEXT NOT NULL, verified_at TEXT, metadata_json TEXT NOT NULL DEFAULT '{}', content_hash TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS memory_anchor_links (memory_id TEXT NOT NULL, memory_revision INTEGER NOT NULL, anchor_id TEXT NOT NULL REFERENCES source_anchors(id), role TEXT NOT NULL, PRIMARY KEY(memory_id, memory_revision, anchor_id, role), FOREIGN KEY(memory_id, memory_revision) REFERENCES memory_revisions(memory_id, revision));
         CREATE TABLE IF NOT EXISTS assurance_decisions (id TEXT PRIMARY KEY, memory_id TEXT NOT NULL, memory_revision INTEGER NOT NULL, outcome TEXT NOT NULL, anchor_set_hash TEXT NOT NULL, actor_kind TEXT NOT NULL, actor_id TEXT NOT NULL, rationale TEXT NOT NULL, decided_at TEXT NOT NULL, FOREIGN KEY(memory_id, memory_revision) REFERENCES memory_revisions(memory_id, revision));
         CREATE TABLE IF NOT EXISTS memory_relations (from_memory_id TEXT NOT NULL REFERENCES memory_artifacts(id), relation_type TEXT NOT NULL, to_memory_id TEXT NOT NULL REFERENCES memory_artifacts(id), created_at TEXT NOT NULL, PRIMARY KEY(from_memory_id, relation_type, to_memory_id));
         CREATE TABLE IF NOT EXISTS capture_receipts (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, idempotency_key TEXT NOT NULL, payload_hash TEXT NOT NULL, accepted_memory_id TEXT NOT NULL, rejection_code TEXT, received_at TEXT NOT NULL, UNIQUE(agent_id, idempotency_key));
         CREATE TABLE IF NOT EXISTS feedback_events (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, subject_type TEXT NOT NULL, subject_id TEXT NOT NULL, signal TEXT NOT NULL, safe_note TEXT, created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS feedback_receipts (agent_id TEXT NOT NULL, idempotency_key TEXT NOT NULL, payload_hash TEXT NOT NULL, feedback_id TEXT NOT NULL REFERENCES feedback_events(id), received_at TEXT NOT NULL, PRIMARY KEY(agent_id,idempotency_key));
         CREATE TABLE IF NOT EXISTS source_events (id TEXT PRIMARY KEY, anchor_id TEXT NOT NULL REFERENCES source_anchors(id), event_type TEXT NOT NULL, new_digest TEXT, actor TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS lifecycle_events (id TEXT PRIMARY KEY, memory_id TEXT NOT NULL REFERENCES memory_artifacts(id), event_type TEXT NOT NULL, reason TEXT NOT NULL, actor TEXT NOT NULL, created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS memory_navigation (memory_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, kind TEXT NOT NULL, scope_type TEXT NOT NULL, scope_key TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL, revision INTEGER NOT NULL, updated_at TEXT NOT NULL, usage_count INTEGER NOT NULL DEFAULT 0, source_epoch TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS memory_navigation_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), source_epoch TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS auto_memory_hook_runs (agent_id TEXT PRIMARY KEY, last_run_at TEXT NOT NULL, last_outcome TEXT NOT NULL, config_version INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS auto_memory_prompt_signals (agent_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL, signal TEXT NOT NULL, config_version INTEGER NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(agent_id,session_id,turn_id));
         CREATE TABLE IF NOT EXISTS auto_memory_trigger_receipts (receipt_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL, event_hash TEXT NOT NULL, outcome TEXT NOT NULL, config_version INTEGER NOT NULL, created_at TEXT NOT NULL, UNIQUE(agent_id,session_id,turn_id));
         CREATE TABLE IF NOT EXISTS auto_memory_submission_receipts (receipt_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, trigger_receipt_id TEXT NOT NULL, candidate_index INTEGER NOT NULL CHECK(candidate_index = 0), payload_hash TEXT NOT NULL, memory_id TEXT REFERENCES memory_artifacts(id), outcome TEXT NOT NULL, policy_version TEXT NOT NULL, reason TEXT NOT NULL, config_version INTEGER NOT NULL, received_at TEXT NOT NULL, UNIQUE(agent_id,trigger_receipt_id,candidate_index));",
    ).map_err(map_db)?;
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(memory_id UNINDEXED,revision UNINDEXED,search_text,tokenize='unicode61');",
        )
        .map_err(map_db)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_db)?;
    let row: Option<(String, String, i64)> = transaction
        .query_row(
            "SELECT database_kind, agent_id, schema_version FROM art_meta LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(map_db)?;
    match row {
        None => {
            transaction.execute("INSERT INTO art_meta(database_kind, agent_id, schema_version) VALUES ('agent-vault', ?1, ?2)", params![agent_id.as_str(), SCHEMA_VERSION]).map_err(map_db)?;
        }
        Some((_kind, _owner, version)) if version > SCHEMA_VERSION => {
            return Err(ArtError::SchemaTooNew);
        }
        Some((kind, owner, _)) if kind != "agent-vault" || owner != agent_id.as_str() => {
            return Err(ArtError::IdentityMismatch);
        }
        Some((_kind, _owner, version)) if version < SCHEMA_VERSION => {
            let rows = {
                let mut statement = transaction
                    .prepare("SELECT id,artifact_json FROM memory_artifacts ORDER BY id")
                    .map_err(map_db)?;
                statement
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(map_db)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_db)?
            };
            for (memory_id, artifact_json) in rows {
                let memory: MemoryArtifact = serde_json::from_str(&artifact_json)
                    .map_err(|error| ArtError::Internal(error.to_string()))?;
                transaction
                    .execute(
                        "UPDATE memory_artifacts SET valid_from=?2,valid_until=?3,review_after=?4 WHERE id=?1",
                        params![
                            memory_id,
                            memory.valid_from.map(|value| value.to_rfc3339()),
                            memory.valid_until.map(|value| value.to_rfc3339()),
                            memory.review_after.map(|value| value.to_rfc3339()),
                        ],
                    )
                    .map_err(map_db)?;
            }
            transaction
                .execute("UPDATE art_meta SET schema_version=?1", [SCHEMA_VERSION])
                .map_err(map_db)?;
        }
        Some(_) => {}
    }
    intake::migrate_intake(&transaction, agent_id)?;
    transaction.commit().map_err(map_db)
}

fn scope_type(scope: &art_domain::memory::MemoryScope) -> &'static str {
    match scope {
        art_domain::memory::MemoryScope::Session(_) => "session",
        art_domain::memory::MemoryScope::Repository(_) => "repository",
        art_domain::memory::MemoryScope::Workspace(_) => "workspace",
        art_domain::memory::MemoryScope::Machine(_) => "machine",
        art_domain::memory::MemoryScope::User(_) => "user",
    }
}

fn scope_key(scope: &art_domain::memory::MemoryScope) -> &str {
    match scope {
        art_domain::memory::MemoryScope::Session(value)
        | art_domain::memory::MemoryScope::Repository(value)
        | art_domain::memory::MemoryScope::Workspace(value)
        | art_domain::memory::MemoryScope::Machine(value)
        | art_domain::memory::MemoryScope::User(value) => value,
    }
}

fn parse_anchor_kind(value: &str) -> ArtResult<AnchorKind> {
    match value {
        "hostsessionrange" | "host_session_range" => Ok(AnchorKind::HostSessionRange),
        "userstatement" | "user_statement" => Ok(AnchorKind::UserStatement),
        "filesnapshot" | "file_snapshot" => Ok(AnchorKind::FileSnapshot),
        "gitobject" | "git_object" => Ok(AnchorKind::GitObject),
        "commandreceipt" | "command_receipt" => Ok(AnchorKind::CommandReceipt),
        "testreceipt" | "test_receipt" => Ok(AnchorKind::TestReceipt),
        "logexcerpt" | "log_excerpt" => Ok(AnchorKind::LogExcerpt),
        "externaldocument" | "external_document" => Ok(AnchorKind::ExternalDocument),
        _ => Err(ArtError::Internal("unknown anchor kind".into())),
    }
}

fn parse_sensitivity(value: &str) -> ArtResult<Sensitivity> {
    match value {
        "private" => Ok(Sensitivity::Private),
        "internal" => Ok(Sensitivity::Internal),
        "public" => Ok(Sensitivity::Public),
        _ => Err(ArtError::Internal("unknown sensitivity".into())),
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn capture_payload_hash(memory: &MemoryArtifact, anchors: &[SourceAnchor]) -> String {
    let anchor_hashes = stable_anchor_hashes(anchors);
    art_domain::memory::canonical_json_hash(&serde_json::json!({
        "agent_id":memory.agent_id,
        "title":memory.title,
        "summary":memory.summary,
        "payload":memory.payload,
        "scope":memory.scope,
        "sensitivity":memory.sensitivity,
        "status":memory.status,
        "current_revision":memory.current_revision,
        "current_hash":memory.current_hash,
        "valid_from":memory.valid_from,
        "valid_until":memory.valid_until,
        "review_after":memory.review_after,
        "anchor_hashes":anchor_hashes,
    }))
}

fn revision_payload_hash(
    memory_id: &str,
    expected_revision: u32,
    title: &str,
    summary: &str,
    payload: &art_domain::memory::MemoryPayload,
    anchors: &[SourceAnchor],
    reason: &str,
) -> String {
    let anchor_hashes = stable_anchor_hashes(anchors);
    art_domain::memory::canonical_json_hash(&serde_json::json!({
        "memory_id":memory_id,
        "expected_revision":expected_revision,
        "title":title,
        "summary":summary,
        "payload":payload,
        "anchor_hashes":anchor_hashes,
        "reason":reason,
    }))
}

fn stable_anchor_hashes(anchors: &[SourceAnchor]) -> Vec<String> {
    let mut hashes: Vec<_> = anchors
        .iter()
        .map(|anchor| {
            art_domain::memory::canonical_json_hash(&serde_json::json!({
                "kind":anchor.kind,
                "locator":anchor.locator,
                "source_version":anchor.source_version,
                "source_digest":anchor.source_digest,
                "excerpt_hash":anchor.excerpt_hash,
                "metadata":anchor.metadata,
                "sensitivity":anchor.sensitivity,
            }))
        })
        .collect();
    hashes.sort_unstable();
    hashes
}

fn search_document(memory: &MemoryArtifact) -> String {
    let normalized = format!(
        "{}\n{}\n{}",
        memory.title,
        memory.summary,
        serde_json::to_string(&memory.payload).unwrap_or_default()
    )
    .nfkc()
    .collect::<String>()
    .to_lowercase();
    let cjk: Vec<_> = normalized.chars().filter(|value| is_cjk(*value)).collect();
    let bigrams = cjk
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{normalized}\n{bigrams}")
}

fn fts_expression(terms: &[String]) -> ArtResult<String> {
    let terms: Vec<_> = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty() && term.len() <= 512)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        return Err(ArtError::InvalidInput("recall terms are required".into()));
    }
    Ok(terms.join(" OR "))
}

const fn is_cjk(value: char) -> bool {
    matches!(value as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

#[allow(clippy::needless_pass_by_value)]
fn map_db(error: rusqlite::Error) -> ArtError {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            ArtError::DbBusy
        }
        _ => ArtError::Io(error.to_string()),
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| ArtError::Io(error.to_string()))
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| ArtError::Io(error.to_string()))
}
#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(unix)]
fn file_mode(path: &Path) -> ArtResult<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    Ok(Some(
        fs::metadata(path)
            .map_err(|error| ArtError::Io(error.to_string()))?
            .permissions()
            .mode()
            & 0o777,
    ))
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> ArtResult<Option<u32>> {
    Ok(None)
}
