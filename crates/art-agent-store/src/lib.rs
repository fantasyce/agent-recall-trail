//! Physically isolated per-Agent SQLite Vaults.

use std::{
    fs,
    path::{Path, PathBuf},
};

use art_domain::{
    ArtError, ArtResult,
    agent::AgentId,
    anchor::{AnchorKind, AssuranceDecision, AssuranceOutcome, SourceAnchor, anchor_set_hash},
    memory::{MemoryArtifact, MemoryStatus, Sensitivity},
};
use chrono::Utc;
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

const SCHEMA_VERSION: i64 = 1;

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
        let payload_json =
            serde_json::to_string(memory).map_err(|error| ArtError::Internal(error.to_string()))?;
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

        transaction
            .execute(
                "INSERT INTO memory_artifacts (id, agent_id, kind, status, title, summary, scope_type, scope_key, sensitivity, current_revision, current_hash, artifact_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![memory.id, self.agent_id.as_str(), memory.payload.kind_name(), format!("{:?}", memory.status).to_lowercase(), memory.title, memory.summary, scope_type(&memory.scope), scope_key(&memory.scope), format!("{:?}", memory.sensitivity).to_lowercase(), memory.current_revision, memory.current_hash, payload_json, memory.created_at.to_rfc3339(), memory.updated_at.to_rfc3339()],
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
                params![memory.id, revision.revision, serde_json::to_string(&revision.payload).map_err(|error| ArtError::Internal(error.to_string()))?, revision.content_hash, self.agent_id.as_str(), revision.changed_at.to_rfc3339(), revision.reason],
            ).map_err(map_db)?;
        }
        for anchor in anchors {
            transaction.execute(
                "INSERT INTO source_anchors (id, owner_agent_id, kind, locator, source_version, source_digest, excerpt, excerpt_hash, sensitivity, observed_at, metadata_json, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![anchor.id, self.agent_id.as_str(), format!("{:?}", anchor.kind).to_lowercase(), anchor.locator, anchor.source_version, anchor.source_digest, anchor.excerpt, anchor.excerpt_hash, format!("{:?}", anchor.sensitivity).to_lowercase(), anchor.observed_at.to_rfc3339(), serde_json::to_string(&anchor.metadata).map_err(|error| ArtError::Internal(error.to_string()))?, anchor.content_hash],
            ).map_err(map_db)?;
            transaction.execute(
                "INSERT INTO memory_anchor_links (memory_id, memory_revision, anchor_id, role) VALUES (?1, ?2, ?3, 'evidence')",
                params![memory.id, memory.current_revision, anchor.id],
            ).map_err(map_db)?;
        }
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

    pub fn rebuild_navigation(&self) -> ArtResult<u64> {
        let source_epoch = self.index_epoch()?;
        let now = Utc::now();
        let eligible: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|memory| {
                memory.status == MemoryStatus::Active
                    && memory.valid_from.is_none_or(|start| start <= now)
                    && memory.valid_until.is_none_or(|end| end > now)
            })
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

    pub fn search_candidates(&self, terms: &[String]) -> ArtResult<Vec<MemoryArtifact>> {
        self.search_ranked_candidates(terms, 512).map(|ranked| {
            ranked
                .into_iter()
                .map(|candidate| candidate.artifact)
                .collect()
        })
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
            migration_checksum: hex_digest(b"art.agent-vault.schema.v1"),
            database_kind,
            bound_agent_id,
            integrity_ok: integrity == "ok",
            foreign_key_violations,
            journal_mode,
            memory_count,
            search_index_count,
            search_index_aligned: memory_count == search_index_count,
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
            "UPDATE memory_artifacts SET status=?2,artifact_json=?3,updated_at=?4,superseded_by=COALESCE(?5,superseded_by),title=?6,summary=?7,current_revision=?8,current_hash=?9 WHERE id=?1",
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
            ],
        )
        .map_err(map_db)?;
    Ok(())
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
         CREATE TABLE IF NOT EXISTS memory_navigation_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), source_epoch TEXT NOT NULL);",
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
        Some(_) => {}
    }
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
