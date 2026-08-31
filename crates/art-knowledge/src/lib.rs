//! Human-reviewed proposals and immutable Knowledge Editions.

pub mod backup;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use art_domain::{
    ArtError, ArtResult,
    agent::AgentId,
    knowledge::{
        KnowledgeDraft, KnowledgeProposal, ProposalSourceLock, ProposalStatus, ReviewActor,
        RiskLevel, proposal_source_set_hash,
    },
    memory::canonical_json_hash,
};
use chrono::Utc;
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;

type ProposalRow = (u32, String, String, String, String, String, String, String);
type EditionRow = (String, u32, String, String, String, String, String, String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditionRecord {
    pub edition_id: String,
    pub knowledge_key: String,
    pub edition_number: u32,
    pub title: String,
    pub markdown_path: PathBuf,
    pub manifest_path: PathBuf,
    pub markdown_sha256: String,
    pub manifest_sha256: String,
    pub published_at: String,
}

#[derive(Debug, Clone)]
pub struct RankedEditionCandidate {
    pub edition: EditionRecord,
    pub lexical_rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeNavigationEntry {
    pub edition_id: String,
    pub knowledge_key: String,
    pub edition_number: u32,
    pub title: String,
    pub applicability: String,
    pub published_at: String,
    pub current: bool,
    pub usage_count: u64,
    pub source_epoch: String,
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct KnowledgeDiagnostics {
    pub integrity_ok: bool,
    pub foreign_key_violations: u64,
    pub journal_mode: String,
    pub proposal_count: u64,
    pub stale_proposal_count: u64,
    pub pending_publish_intents: u64,
    pub projection_count: u64,
    pub current_edition_count: u64,
    pub search_index_count: u64,
    pub search_index_aligned: bool,
    pub navigation_count: u64,
    pub navigation_aligned: bool,
    pub manifest_files_verified: u64,
    pub event_files_verified: u64,
    pub projection_hashes_ok: bool,
    pub wal_bytes: u64,
    pub control_file_mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SharedManifest {
    schema: String,
    edition_id: String,
    knowledge_key: String,
    edition_number: u32,
    title: String,
    markdown_body_sha256: String,
    source_set_hash: String,
    source_commitments: Vec<String>,
    review_receipt_hash: String,
    published_at: String,
    generator: String,
}

#[derive(Debug, Clone)]
pub struct KnowledgeVault {
    root: PathBuf,
    control_db: PathBuf,
    commitment_key: [u8; 32],
}

impl KnowledgeVault {
    pub fn open(root: impl AsRef<Path>, commitment_key: [u8; 32]) -> ArtResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("editions")).map_err(io_error)?;
        fs::create_dir_all(root.join(".art/events")).map_err(io_error)?;
        set_private_directory(&root)?;
        set_private_directory(&root.join("editions"))?;
        set_private_directory(&root.join(".art"))?;
        set_private_directory(&root.join(".art/events"))?;
        let control_db = root.join("art-control.sqlite3");
        let connection = Connection::open(&control_db).map_err(db_error)?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS knowledge_proposals (id TEXT PRIMARY KEY, revision INTEGER NOT NULL, status TEXT NOT NULL, author_agent_id TEXT NOT NULL, draft_json TEXT NOT NULL, sources_json TEXT NOT NULL, source_set_hash TEXT NOT NULL, idempotency_key TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS proposal_reviews (id TEXT PRIMARY KEY, proposal_id TEXT NOT NULL, proposal_revision INTEGER NOT NULL, source_set_hash TEXT NOT NULL, decision TEXT NOT NULL, actor TEXT NOT NULL, reason TEXT NOT NULL, decided_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS publish_intents (id TEXT PRIMARY KEY, proposal_id TEXT NOT NULL, proposal_revision INTEGER NOT NULL, edition_id TEXT NOT NULL, target_dir TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS event_intents (id TEXT PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, edition_id TEXT NOT NULL, target_path TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS edition_projections (edition_id TEXT PRIMARY KEY, knowledge_key TEXT NOT NULL, edition_number INTEGER NOT NULL, title TEXT NOT NULL, markdown_path TEXT NOT NULL, manifest_path TEXT NOT NULL, markdown_sha256 TEXT NOT NULL, manifest_sha256 TEXT NOT NULL, published_at TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0, current INTEGER NOT NULL DEFAULT 1);
             CREATE TABLE IF NOT EXISTS knowledge_events (event_id TEXT PRIMARY KEY, event_hash TEXT NOT NULL, schema TEXT NOT NULL, edition_id TEXT NOT NULL, applied_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS knowledge_navigation (edition_id TEXT PRIMARY KEY, knowledge_key TEXT NOT NULL, edition_number INTEGER NOT NULL, title TEXT NOT NULL, applicability TEXT NOT NULL, published_at TEXT NOT NULL, current INTEGER NOT NULL, usage_count INTEGER NOT NULL DEFAULT 0, source_epoch TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS knowledge_navigation_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), source_epoch TEXT NOT NULL);
             CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(edition_id UNINDEXED,search_text,tokenize='unicode61');
             CREATE INDEX IF NOT EXISTS edition_current ON edition_projections(knowledge_key, current, revoked);",
        ).map_err(db_error)?;
        drop(connection);
        set_private_permissions(&control_db)?;
        let vault = Self {
            root,
            control_db,
            commitment_key,
        };
        vault.recover_publish_intents()?;
        vault.recover_event_intents()?;
        vault.reconcile_events()?;
        Ok(vault)
    }

    pub fn propose(
        &self,
        author: &AgentId,
        draft: KnowledgeDraft,
        sources: Vec<ProposalSourceLock>,
        idempotency_key: &str,
    ) -> ArtResult<KnowledgeProposal> {
        validate_draft(&draft)?;
        if sources.is_empty() {
            return Err(ArtError::SourceRequired);
        }
        if idempotency_key.trim().is_empty() {
            return Err(ArtError::InvalidInput("idempotency key is required".into()));
        }
        if sources.iter().any(|source| {
            source.source_content_hash.len() != 64 || source.source_revision == Some(0)
        }) {
            return Err(ArtError::InvalidInput(
                "source lock must contain exact revision and hash".into(),
            ));
        }
        let connection = self.connection()?;
        let existing: Option<String> = connection
            .query_row(
                "SELECT id FROM knowledge_proposals WHERE idempotency_key=?1",
                [idempotency_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(id) = existing {
            let existing = self.proposal(&id)?;
            let requested_hash = canonical_json_hash(&serde_json::json!({
                "author":author,
                "draft":draft,
                "sources":sources,
            }));
            let existing_hash = canonical_json_hash(&serde_json::json!({
                "author":existing.author_agent_id,
                "draft":existing.draft,
                "sources":existing.sources,
            }));
            if requested_hash != existing_hash {
                return Err(ArtError::DuplicateConflict);
            }
            return Ok(existing);
        }
        let now = Utc::now();
        let proposal = KnowledgeProposal {
            id: format!("artp_{}", Ulid::new()),
            revision: 1,
            status: ProposalStatus::Submitted,
            author_agent_id: author.clone(),
            draft,
            source_set_hash: proposal_source_set_hash(&sources),
            sources,
            created_at: now,
            updated_at: now,
        };
        connection.execute("INSERT INTO knowledge_proposals(id, revision, status, author_agent_id, draft_json, sources_json, source_set_hash, idempotency_key, created_at, updated_at) VALUES (?1,?2,'submitted',?3,?4,?5,?6,?7,?8,?8)", params![proposal.id, proposal.revision, author.as_str(), serde_json::to_string(&proposal.draft).map_err(internal_error)?, serde_json::to_string(&proposal.sources).map_err(internal_error)?, proposal.source_set_hash, idempotency_key, now.to_rfc3339()]).map_err(db_error)?;
        Ok(proposal)
    }

    pub fn proposal(&self, id: &str) -> ArtResult<KnowledgeProposal> {
        let connection = self.connection()?;
        let row: Option<ProposalRow> = connection.query_row("SELECT revision,status,author_agent_id,draft_json,sources_json,source_set_hash,created_at,updated_at FROM knowledge_proposals WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?))).optional().map_err(db_error)?;
        let (revision, status, author, draft, sources, source_set_hash, created, updated) =
            row.ok_or(ArtError::NotFound)?;
        Ok(KnowledgeProposal {
            id: id.into(),
            revision,
            status: parse_status(&status)?,
            author_agent_id: author.parse()?,
            draft: serde_json::from_str(&draft).map_err(internal_error)?,
            sources: serde_json::from_str(&sources).map_err(internal_error)?,
            source_set_hash,
            created_at: chrono::DateTime::parse_from_rfc3339(&created)
                .map_err(internal_error)?
                .with_timezone(&Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&updated)
                .map_err(internal_error)?
                .with_timezone(&Utc),
        })
    }

    pub fn list_proposals(&self) -> ArtResult<Vec<KnowledgeProposal>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT id FROM knowledge_proposals ORDER BY updated_at DESC")
            .map_err(db_error)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        ids.map(|id| self.proposal(&id.map_err(db_error)?))
            .collect()
    }

    pub fn approve(
        &self,
        id: &str,
        revision: u32,
        actor: ReviewActor,
        reason: &str,
    ) -> ArtResult<()> {
        self.review(id, revision, actor, "approved", reason)
    }

    pub fn review(
        &self,
        id: &str,
        revision: u32,
        actor: ReviewActor,
        decision: &str,
        reason: &str,
    ) -> ArtResult<()> {
        let actor_id = match actor {
            ReviewActor::Human(id) if !id.trim().is_empty() => id,
            ReviewActor::Human(_) => {
                return Err(ArtError::InvalidInput("review actor is required".into()));
            }
            _ => {
                return Err(ArtError::PermissionDenied(
                    "only a local human may approve knowledge".into(),
                ));
            }
        };
        let proposal = self.proposal(id)?;
        if proposal.revision != revision || proposal.status == ProposalStatus::Stale {
            return Err(ArtError::SourceStale);
        }
        if reason.trim().is_empty() {
            return Err(ArtError::InvalidInput("review reason is required".into()));
        }
        let next_status = match decision {
            "approved" => "approved",
            "changes_requested" => "changes_requested",
            "rejected" => "rejected",
            _ => {
                return Err(ArtError::InvalidInput("invalid review decision".into()));
            }
        };
        let connection = self.connection()?;
        if decision == "approved" {
            let unsafe_body = proposal.draft.markdown.to_ascii_lowercase();
            if unsafe_body.contains("ignore previous") || unsafe_body.contains("忽略之前") {
                return Err(ArtError::PermissionDenied(
                    "instruction-like knowledge requires revision before approval".into(),
                ));
            }
        }
        let needs_second_human = decision == "approved"
            && matches!(proposal.draft.risk, RiskLevel::Elevated | RiskLevel::High)
            && proposal
                .sources
                .iter()
                .map(|source| &source.source_content_hash)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                < 2;
        let prior_independent_approval: bool = if needs_second_human {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM proposal_reviews WHERE proposal_id=?1 AND proposal_revision=?2 AND source_set_hash=?3 AND decision='approved' AND actor!=?4)",
                params![id,revision,proposal.source_set_hash,actor_id],
                |row| row.get(0),
            ).map_err(db_error)?
        } else {
            true
        };
        connection.execute("INSERT INTO proposal_reviews(id,proposal_id,proposal_revision,source_set_hash,decision,actor,reason,decided_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![format!("artr_{}",Ulid::new()),id,revision,proposal.source_set_hash,decision,actor_id,reason,Utc::now().to_rfc3339()]).map_err(db_error)?;
        let next_status = if needs_second_human && !prior_independent_approval {
            "under_review"
        } else {
            next_status
        };
        connection
            .execute(
                "UPDATE knowledge_proposals SET status=?2,updated_at=?3 WHERE id=?1",
                params![id, next_status, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn mark_source_changed(&self, id: &str, new_hash: &str) -> ArtResult<()> {
        if new_hash.len() != 64 {
            return Err(ArtError::InvalidInput("source hash must be SHA-256".into()));
        }
        let connection = self.connection()?;
        if connection
            .execute(
                "UPDATE knowledge_proposals SET status='stale',updated_at=?2 WHERE id=?1",
                params![id, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?
            == 0
        {
            return Err(ArtError::NotFound);
        }
        Ok(())
    }

    pub fn publish(&self, id: &str, revision: u32, confirm: bool) -> ArtResult<EditionRecord> {
        if !confirm {
            return Err(ArtError::PermissionDenied(
                "publishing requires explicit confirmation".into(),
            ));
        }
        let proposal = self.proposal(id)?;
        if proposal.status == ProposalStatus::Stale {
            return Err(ArtError::SourceStale);
        }
        if proposal.status != ProposalStatus::Approved || proposal.revision != revision {
            return Err(ArtError::InvalidStateTransition);
        }
        let mut connection = self.connection()?;
        let review_hash: Option<String> = connection.query_row("SELECT source_set_hash FROM proposal_reviews WHERE proposal_id=?1 AND proposal_revision=?2 AND decision='approved' ORDER BY decided_at DESC LIMIT 1", params![id,revision], |row| row.get(0)).optional().map_err(db_error)?;
        if review_hash.as_deref() != Some(&proposal.source_set_hash) {
            return Err(ArtError::SourceStale);
        }
        let edition_number: u32 = connection.query_row("SELECT COALESCE(MAX(edition_number),0)+1 FROM edition_projections WHERE knowledge_key=?1", [&proposal.draft.knowledge_key], |row| row.get(0)).map_err(db_error)?;
        let edition_id = format!("arke_{}", Ulid::new());
        let target_dir = self
            .root
            .join("editions")
            .join(&proposal.draft.knowledge_key);
        ensure_target(&self.root, &target_dir)?;
        fs::create_dir_all(&target_dir).map_err(io_error)?;
        set_private_directory(&target_dir)?;
        let intent_id = format!("arti_{}", Ulid::new());
        connection.execute("INSERT INTO publish_intents(id,proposal_id,proposal_revision,edition_id,target_dir,state,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'prepared',?6,?6)", params![intent_id,id,revision,edition_id,target_dir.to_string_lossy(),Utc::now().to_rfc3339()]).map_err(db_error)?;
        let published_at = Utc::now().to_rfc3339();
        let body_hash = hex_digest(proposal.draft.markdown.as_bytes());
        let source_commitments = proposal
            .sources
            .iter()
            .map(|source| self.commitment(source))
            .collect();
        let manifest = SharedManifest {
            schema: "art.knowledge.edition.v1".into(),
            edition_id: edition_id.clone(),
            knowledge_key: proposal.draft.knowledge_key.clone(),
            edition_number,
            title: proposal.draft.title.clone(),
            markdown_body_sha256: body_hash,
            source_set_hash: proposal.source_set_hash.clone(),
            source_commitments,
            review_receipt_hash: canonical_json_hash(&serde_json::json!([
                id,
                revision,
                proposal.source_set_hash
            ])),
            published_at: published_at.clone(),
            generator: env!("CARGO_PKG_VERSION").into(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(internal_error)?;
        let manifest_sha256 = hex_digest(&manifest_bytes);
        let stem = format!("{edition_number}-{edition_id}");
        let manifest_path = target_dir.join(format!("{stem}.json"));
        let markdown_path = target_dir.join(format!("{stem}.md"));
        let markdown = format!(
            "---\ntitle: {}\ntype: art-knowledge\nknowledge_key: {}\nedition_id: {}\nedition_number: {}\nstatus: published\nsensitivity: {:?}\npublished_at: {}\nmanifest: {}\nmanifest_sha256: {}\n---\n\n## Applicability\n\n{}\n\n## Knowledge\n\n{}\n",
            proposal.draft.title,
            proposal.draft.knowledge_key,
            edition_id,
            edition_number,
            proposal.draft.sensitivity,
            published_at,
            manifest_path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default(),
            manifest_sha256,
            proposal.draft.applicability,
            proposal.draft.markdown
        );
        let markdown_sha256 = hex_digest(markdown.as_bytes());
        atomic_create(&manifest_path, &manifest_bytes)?;
        if let Err(error) = atomic_create(&markdown_path, markdown.as_bytes()) {
            let _ = fs::remove_file(&manifest_path);
            return Err(error);
        }
        let stored_markdown_path = self.portable_projection_path(&markdown_path)?;
        let stored_manifest_path = self.portable_projection_path(&manifest_path)?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute(
                "UPDATE edition_projections SET current=0 WHERE knowledge_key=?1",
                [&proposal.draft.knowledge_key],
            )
            .map_err(db_error)?;
        transaction.execute("INSERT INTO edition_projections(edition_id,knowledge_key,edition_number,title,markdown_path,manifest_path,markdown_sha256,manifest_sha256,published_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![edition_id,proposal.draft.knowledge_key,edition_number,proposal.draft.title,stored_markdown_path,stored_manifest_path,markdown_sha256,manifest_sha256,published_at]).map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO knowledge_fts(edition_id,search_text) VALUES (?1,?2)",
                params![edition_id, search_document(&markdown)],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE knowledge_proposals SET status='materialized',updated_at=?2 WHERE id=?1",
                params![id, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE publish_intents SET state='committed',updated_at=?2 WHERE id=?1",
                params![intent_id, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        self.read_including_revoked(&edition_id)
    }

    pub fn current(&self, key: &str) -> ArtResult<EditionRecord> {
        let connection = self.connection()?;
        let id: Option<String> = connection.query_row("SELECT edition_id FROM edition_projections WHERE knowledge_key=?1 AND current=1 AND revoked=0", [key], |row| row.get(0)).optional().map_err(db_error)?;
        self.read(&id.ok_or(ArtError::NotFound)?)
    }

    pub fn read(&self, id: &str) -> ArtResult<EditionRecord> {
        let connection = self.connection()?;
        let revoked: Option<i64> = connection
            .query_row(
                "SELECT revoked FROM edition_projections WHERE edition_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if revoked != Some(0) {
            return Err(ArtError::NotFound);
        }
        self.read_including_revoked(id)
    }

    pub fn list_current(&self) -> ArtResult<Vec<EditionRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT edition_id FROM edition_projections WHERE current=1 AND revoked=0 ORDER BY published_at DESC").map_err(db_error)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        ids.map(|id| self.read(&id.map_err(db_error)?)).collect()
    }

    pub fn search_ranked_candidates(
        &self,
        terms: &[String],
        limit: usize,
    ) -> ArtResult<Vec<RankedEditionCandidate>> {
        if !(1..=2_048).contains(&limit) {
            return Err(ArtError::InvalidInput(
                "candidate limit must be 1..=2048".into(),
            ));
        }
        let connection = self.connection()?;
        let expression = fts_expression(terms)?;
        let mut statement = connection
            .prepare(
                "SELECT p.edition_id FROM knowledge_fts f JOIN edition_projections p ON p.edition_id=f.edition_id WHERE knowledge_fts MATCH ?1 AND p.current=1 AND p.revoked=0 ORDER BY rank,p.published_at DESC,p.edition_id ASC LIMIT ?2",
            )
            .map_err(db_error)?;
        let ids = statement
            .query_map(params![expression, limit], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        ids.enumerate()
            .map(|(index, id)| {
                Ok(RankedEditionCandidate {
                    edition: self.read(&id.map_err(db_error)?)?,
                    lexical_rank: index + 1,
                })
            })
            .collect()
    }

    pub fn search_candidates(&self, terms: &[String]) -> ArtResult<Vec<EditionRecord>> {
        self.search_ranked_candidates(terms, 512).map(|ranked| {
            ranked
                .into_iter()
                .map(|candidate| candidate.edition)
                .collect()
        })
    }

    pub fn rebuild_search_index(&self) -> ArtResult<u64> {
        let records = self.list_all_projection_records()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM knowledge_fts", [])
            .map_err(db_error)?;
        for record in &records {
            let markdown = fs::read_to_string(&record.markdown_path).map_err(io_error)?;
            transaction
                .execute(
                    "INSERT INTO knowledge_fts(edition_id,search_text) VALUES (?1,?2)",
                    params![record.edition_id, search_document(&markdown)],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)?;
        u64::try_from(records.len()).map_err(internal_error)
    }

    pub fn index_epoch(&self) -> ArtResult<String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT edition_id,markdown_sha256,manifest_sha256,revoked,current FROM edition_projections ORDER BY edition_id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(db_error)?;
        let mut hasher = Sha256::new();
        for row in rows {
            let (id, markdown, manifest, revoked, current) = row.map_err(db_error)?;
            hasher.update(id);
            hasher.update(markdown);
            hasher.update(manifest);
            hasher.update(revoked.to_le_bytes());
            hasher.update(current.to_le_bytes());
        }
        let mut events = connection
            .prepare("SELECT event_id,event_hash FROM knowledge_events ORDER BY event_id")
            .map_err(db_error)?;
        for row in events
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?
        {
            let (id, hash) = row.map_err(db_error)?;
            hasher.update(id);
            hasher.update(hash);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    pub fn rebuild_navigation(&self) -> ArtResult<u64> {
        let source_epoch = self.index_epoch()?;
        let records = self.list_current()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM knowledge_navigation", [])
            .map_err(db_error)?;
        for record in &records {
            let markdown = fs::read_to_string(&record.markdown_path).map_err(io_error)?;
            let applicability =
                edition_section(&markdown, "Applicability").ok_or(ArtError::IndexDegraded)?;
            transaction.execute(
                "INSERT INTO knowledge_navigation(edition_id,knowledge_key,edition_number,title,applicability,published_at,current,usage_count,source_epoch) VALUES (?1,?2,?3,?4,?5,?6,1,0,?7)",
                params![record.edition_id,record.knowledge_key,record.edition_number,record.title,applicability,record.published_at,source_epoch],
            ).map_err(db_error)?;
        }
        transaction
            .execute(
                "INSERT INTO knowledge_navigation_meta(singleton,source_epoch) VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET source_epoch=excluded.source_epoch",
                [&source_epoch],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        u64::try_from(records.len()).map_err(internal_error)
    }

    pub fn navigation_entries(&self) -> ArtResult<Vec<KnowledgeNavigationEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT edition_id,knowledge_key,edition_number,title,applicability,published_at,current,usage_count,source_epoch FROM knowledge_navigation WHERE current=1 ORDER BY knowledge_key,edition_number DESC,edition_id",
        ).map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(KnowledgeNavigationEntry {
                    edition_id: row.get(0)?,
                    knowledge_key: row.get(1)?,
                    edition_number: row.get(2)?,
                    title: row.get(3)?,
                    applicability: row.get(4)?,
                    published_at: row.get(5)?,
                    current: row.get::<_, i64>(6)? != 0,
                    usage_count: row.get(7)?,
                    source_epoch: row.get(8)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn navigation_aligned(&self) -> ArtResult<bool> {
        let connection = self.connection()?;
        let projected: Option<String> = connection
            .query_row(
                "SELECT source_epoch FROM knowledge_navigation_meta WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let canonical = self.index_epoch()?;
        Ok(projected.as_deref() == Some(canonical.as_str()))
    }

    pub fn revoke(&self, id: &str, reason: &str, confirm: bool) -> ArtResult<()> {
        if !confirm {
            return Err(ArtError::PermissionDenied(
                "revocation requires explicit confirmation".into(),
            ));
        }
        if reason.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "revocation reason is required".into(),
            ));
        }
        let edition = self.read(id)?;
        let event_id = format!("arte_{}", Ulid::new());
        let event = serde_json::json!({"schema":"art.knowledge.revocation.v1","event_id":event_id,"edition_id":id,"reason":reason,"actor":"human:local","revoked_at":Utc::now().to_rfc3339()});
        let mut event = serde_json::to_value(event).map_err(internal_error)?;
        let event_hash = canonical_json_hash(&event);
        event
            .as_object_mut()
            .expect("event object")
            .insert("event_hash".into(), serde_json::Value::String(event_hash));
        self.commit_event(&event_id, id, "revocation", &event)?;
        let _ = edition;
        Ok(())
    }

    pub fn supersede(
        &self,
        edition_id: &str,
        with: &str,
        reason: &str,
        confirm: bool,
    ) -> ArtResult<()> {
        if !confirm {
            return Err(ArtError::PermissionDenied(
                "supersession requires explicit confirmation".into(),
            ));
        }
        if edition_id == with || reason.trim().is_empty() {
            return Err(ArtError::InvalidInput(
                "supersession requires distinct editions and a reason".into(),
            ));
        }
        let previous = self.read_including_revoked(edition_id)?;
        let replacement = self.read(with)?;
        if previous.knowledge_key != replacement.knowledge_key
            || replacement.edition_number <= previous.edition_number
        {
            return Err(ArtError::InvalidStateTransition);
        }
        let event_id = format!("arte_{}", Ulid::new());
        let mut event = serde_json::json!({"schema":"art.knowledge.supersession.v1","event_id":event_id,"edition_id":edition_id,"with":with,"reason":reason,"actor":"human:local","superseded_at":Utc::now().to_rfc3339()});
        let event_hash = canonical_json_hash(&event);
        event
            .as_object_mut()
            .expect("event object")
            .insert("event_hash".into(), serde_json::Value::String(event_hash));
        self.commit_event(&event_id, edition_id, "supersession", &event)
    }

    pub fn rebuild_projection(&self) -> ArtResult<u64> {
        let mut records = Vec::new();
        let editions_root = self.root.join("editions");
        for key_entry in fs::read_dir(&editions_root).map_err(io_error)? {
            let key_path = key_entry.map_err(io_error)?.path();
            if !key_path.is_dir() {
                continue;
            }
            ensure_target(&editions_root, &key_path)?;
            for entry in fs::read_dir(&key_path).map_err(io_error)? {
                let manifest_path = entry.map_err(io_error)?.path();
                if manifest_path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let manifest_bytes = fs::read(&manifest_path).map_err(io_error)?;
                let manifest: SharedManifest =
                    serde_json::from_slice(&manifest_bytes).map_err(internal_error)?;
                if manifest.schema != "art.knowledge.edition.v1" {
                    continue;
                }
                let markdown_path = manifest_path.with_extension("md");
                let markdown = fs::read_to_string(&markdown_path).map_err(io_error)?;
                let knowledge_body = markdown
                    .split_once("## Knowledge\n\n")
                    .map(|(_, body)| body.strip_suffix('\n').unwrap_or(body))
                    .ok_or(ArtError::IndexDegraded)?;
                let manifest_sha256 = hex_digest(&manifest_bytes);
                if hex_digest(knowledge_body.as_bytes()) != manifest.markdown_body_sha256
                    || !markdown.contains(&format!("manifest_sha256: {manifest_sha256}"))
                {
                    return Err(ArtError::IndexDegraded);
                }
                records.push(EditionRecord {
                    edition_id: manifest.edition_id,
                    knowledge_key: manifest.knowledge_key,
                    edition_number: manifest.edition_number,
                    title: manifest.title,
                    markdown_sha256: hex_digest(markdown.as_bytes()),
                    manifest_sha256,
                    markdown_path,
                    manifest_path,
                    published_at: manifest.published_at,
                });
            }
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM edition_projections", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM knowledge_fts", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM knowledge_events", [])
            .map_err(db_error)?;
        for record in &records {
            let stored_markdown_path = self.portable_projection_path(&record.markdown_path)?;
            let stored_manifest_path = self.portable_projection_path(&record.manifest_path)?;
            transaction.execute("INSERT INTO edition_projections(edition_id,knowledge_key,edition_number,title,markdown_path,manifest_path,markdown_sha256,manifest_sha256,published_at,revoked,current) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0)", params![record.edition_id,record.knowledge_key,record.edition_number,record.title,stored_markdown_path,stored_manifest_path,record.markdown_sha256,record.manifest_sha256,record.published_at]).map_err(db_error)?;
            let markdown = fs::read_to_string(&record.markdown_path).map_err(io_error)?;
            transaction
                .execute(
                    "INSERT INTO knowledge_fts(edition_id,search_text) VALUES (?1,?2)",
                    params![record.edition_id, search_document(&markdown)],
                )
                .map_err(db_error)?;
        }
        let events_root = self.root.join(".art/events");
        for entry in fs::read_dir(&events_root).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let mut event: serde_json::Value =
                serde_json::from_slice(&fs::read(path).map_err(io_error)?)
                    .map_err(internal_error)?;
            let stored_hash = event
                .as_object_mut()
                .and_then(|object| object.remove("event_hash"))
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or(ArtError::IndexDegraded)?;
            if canonical_json_hash(&event) != stored_hash {
                return Err(ArtError::IndexDegraded);
            }
            let event_id = event["event_id"].as_str().ok_or(ArtError::IndexDegraded)?;
            let schema = event["schema"].as_str().ok_or(ArtError::IndexDegraded)?;
            let edition_id = event["edition_id"]
                .as_str()
                .ok_or(ArtError::IndexDegraded)?;
            if schema == "art.knowledge.revocation.v1" {
                transaction
                    .execute(
                        "UPDATE edition_projections SET revoked=1,current=0 WHERE edition_id=?1",
                        [edition_id],
                    )
                    .map_err(db_error)?;
            } else if schema == "art.knowledge.supersession.v1" {
                transaction
                    .execute(
                        "UPDATE edition_projections SET current=0 WHERE edition_id=?1",
                        [edition_id],
                    )
                    .map_err(db_error)?;
            } else {
                return Err(ArtError::IndexDegraded);
            }
            transaction.execute(
                "INSERT INTO knowledge_events(event_id,event_hash,schema,edition_id,applied_at) VALUES (?1,?2,?3,?4,?5)",
                params![event_id,stored_hash,schema,edition_id,Utc::now().to_rfc3339()],
            ).map_err(db_error)?;
        }
        transaction.execute("UPDATE edition_projections AS candidate SET current=1 WHERE revoked=0 AND edition_number=(SELECT MAX(newest.edition_number) FROM edition_projections AS newest WHERE newest.knowledge_key=candidate.knowledge_key AND newest.revoked=0)", []).map_err(db_error)?;
        transaction.execute("UPDATE publish_intents SET state='committed',updated_at=?1 WHERE edition_id IN (SELECT edition_id FROM edition_projections)", [Utc::now().to_rfc3339()]).map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        u64::try_from(records.len()).map_err(internal_error)
    }

    #[doc(hidden)]
    pub fn test_only_clear_projection(&self) -> ArtResult<()> {
        self.connection()?
            .execute("DELETE FROM edition_projections", [])
            .map_err(db_error)?;
        Ok(())
    }

    pub fn pending_recoveries(&self) -> ArtResult<u64> {
        let connection = self.connection()?;
        let publications: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM publish_intents WHERE state!='committed'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let events: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM event_intents WHERE state!='committed'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        Ok(publications + events)
    }

    pub fn checkpoint_wal(&self) -> ArtResult<()> {
        self.connection()?
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(db_error)
    }

    /// Recovers publish operations that crossed the SQLite/filesystem boundary.
    /// Fully materialized and hash-valid editions are projected; partial files are
    /// quarantined and remain explicitly recoverable for a human retry.
    pub fn recover_publish_intents(&self) -> ArtResult<u64> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,edition_id,target_dir FROM publish_intents WHERE state NOT IN ('committed','recoverable') ORDER BY created_at",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    PathBuf::from(row.get::<_, String>(2)?),
                ))
            })
            .map_err(db_error)?;
        let intents: Vec<_> = rows.collect::<Result<_, _>>().map_err(db_error)?;
        drop(statement);
        drop(connection);

        let mut recovered = 0_u64;
        for (intent_id, edition_id, target_dir) in intents {
            ensure_target(&self.root, &target_dir)?;
            let mut markdown = None;
            let mut manifest = None;
            let mut temporary = Vec::new();
            if target_dir.exists() {
                for entry in fs::read_dir(&target_dir).map_err(io_error)? {
                    let path = entry.map_err(io_error)?.path();
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("");
                    if !name.contains(&edition_id) {
                        continue;
                    }
                    match path.extension().and_then(|value| value.to_str()) {
                        Some("md") => markdown = Some(path),
                        Some("json") => manifest = Some(path),
                        Some("tmp") => temporary.push(path),
                        _ => {}
                    }
                }
            }
            if markdown.is_some() && manifest.is_some() {
                self.rebuild_projection()?;
                recovered += 1;
                continue;
            }

            let recovery_dir = self.root.join(".art/recovery").join(&intent_id);
            ensure_target(&self.root, &recovery_dir)?;
            fs::create_dir_all(&recovery_dir).map_err(io_error)?;
            set_private_directory(&self.root.join(".art/recovery"))?;
            set_private_directory(&recovery_dir)?;
            for path in markdown.into_iter().chain(manifest).chain(temporary) {
                let target = recovery_dir.join(
                    path.file_name()
                        .ok_or_else(|| ArtError::PathConflict("invalid recovery path".into()))?,
                );
                fs::rename(path, target).map_err(io_error)?;
            }
            self.connection()?
                .execute(
                    "UPDATE publish_intents SET state='recoverable',reason='partial publication quarantined',updated_at=?2 WHERE id=?1",
                    params![intent_id, Utc::now().to_rfc3339()],
                )
                .map_err(db_error)?;
            recovered += 1;
        }
        Ok(recovered)
    }

    pub fn recover_event_intents(&self) -> ArtResult<u64> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,target_path FROM event_intents WHERE state NOT IN ('committed','recoverable') ORDER BY created_at",
        ).map_err(db_error)?;
        let intents: Vec<(String, PathBuf)> = statement
            .query_map([], |row| {
                Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?)))
            })
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        drop(statement);
        drop(connection);
        let mut recovered = 0_u64;
        for (intent_id, target) in intents {
            ensure_target(&self.root, &target)?;
            if target.exists() {
                self.reconcile_events()?;
                self.connection()?
                    .execute(
                        "UPDATE event_intents SET state='committed',updated_at=?2 WHERE id=?1",
                        params![intent_id, Utc::now().to_rfc3339()],
                    )
                    .map_err(db_error)?;
                recovered += 1;
                continue;
            }
            let temporary = target.with_extension(format!(
                "{}.tmp",
                target
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("file")
            ));
            if temporary.exists() {
                let recovery_dir = self.root.join(".art/recovery").join(&intent_id);
                ensure_target(&self.root, &recovery_dir)?;
                fs::create_dir_all(&recovery_dir).map_err(io_error)?;
                set_private_directory(&self.root.join(".art/recovery"))?;
                set_private_directory(&recovery_dir)?;
                let filename = temporary
                    .file_name()
                    .ok_or_else(|| ArtError::PathConflict("invalid recovery path".into()))?;
                fs::rename(&temporary, recovery_dir.join(filename)).map_err(io_error)?;
            }
            self.connection()?.execute(
                "UPDATE event_intents SET state='recoverable',reason='partial lifecycle event quarantined',updated_at=?2 WHERE id=?1",
                params![intent_id,Utc::now().to_rfc3339()],
            ).map_err(db_error)?;
            recovered += 1;
        }
        Ok(recovered)
    }

    /// Applies immutable lifecycle events that reached disk before the control
    /// projection transaction committed, and rejects deleted or altered events.
    pub fn reconcile_events(&self) -> ArtResult<()> {
        let mut files = Vec::new();
        for entry in fs::read_dir(self.root.join(".art/events")).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                files.push(path);
            }
        }
        files.sort();
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let mut file_ids = std::collections::BTreeSet::new();
        for path in files {
            let mut event: serde_json::Value =
                serde_json::from_slice(&fs::read(path).map_err(io_error)?)
                    .map_err(internal_error)?;
            let stored_hash = event
                .as_object_mut()
                .and_then(|object| object.remove("event_hash"))
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or(ArtError::IndexDegraded)?;
            if canonical_json_hash(&event) != stored_hash {
                return Err(ArtError::IndexDegraded);
            }
            let event_id = event["event_id"].as_str().ok_or(ArtError::IndexDegraded)?;
            let schema = event["schema"].as_str().ok_or(ArtError::IndexDegraded)?;
            let edition_id = event["edition_id"]
                .as_str()
                .ok_or(ArtError::IndexDegraded)?;
            file_ids.insert(event_id.to_owned());
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT event_hash FROM knowledge_events WHERE event_id=?1",
                    [event_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if let Some(existing_hash) = existing {
                if existing_hash != stored_hash {
                    return Err(ArtError::IndexDegraded);
                }
                continue;
            }
            match schema {
                "art.knowledge.revocation.v1" => {
                    transaction
                        .execute(
                            "UPDATE edition_projections SET revoked=1,current=0 WHERE edition_id=?1",
                            [edition_id],
                        )
                        .map_err(db_error)?;
                }
                "art.knowledge.supersession.v1" => {
                    let replacement = event["with"].as_str().ok_or(ArtError::IndexDegraded)?;
                    transaction
                        .execute(
                            "UPDATE edition_projections SET current=0 WHERE edition_id=?1",
                            [edition_id],
                        )
                        .map_err(db_error)?;
                    transaction
                        .execute(
                            "UPDATE edition_projections SET current=1 WHERE edition_id=?1 AND revoked=0",
                            [replacement],
                        )
                        .map_err(db_error)?;
                }
                _ => return Err(ArtError::IndexDegraded),
            }
            transaction.execute(
                "INSERT INTO knowledge_events(event_id,event_hash,schema,edition_id,applied_at) VALUES (?1,?2,?3,?4,?5)",
                params![event_id,stored_hash,schema,edition_id,Utc::now().to_rfc3339()],
            ).map_err(db_error)?;
        }
        let mut applied = transaction
            .prepare("SELECT event_id FROM knowledge_events")
            .map_err(db_error)?;
        let applied_ids: Vec<String> = applied
            .query_map([], |row| row.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        drop(applied);
        if applied_ids.iter().any(|id| !file_ids.contains(id)) {
            return Err(ArtError::IndexDegraded);
        }
        transaction.commit().map_err(db_error)
    }

    pub fn diagnostics(&self) -> ArtResult<KnowledgeDiagnostics> {
        let connection = self.connection()?;
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(db_error)?;
        let mut foreign_keys = connection
            .prepare("PRAGMA foreign_key_check")
            .map_err(db_error)?;
        let foreign_key_violations = foreign_keys
            .query_map([], |_| Ok(()))
            .map_err(db_error)?
            .count() as u64;
        drop(foreign_keys);
        let journal_mode = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(db_error)?;
        let count = |query: &str| -> ArtResult<u64> {
            connection
                .query_row(query, [], |row| row.get(0))
                .map_err(db_error)
        };
        let mut projection_hashes_ok = true;
        let mut manifest_files_verified = 0_u64;
        let mut ids = connection
            .prepare("SELECT edition_id FROM edition_projections ORDER BY edition_id")
            .map_err(db_error)?;
        let edition_ids: Vec<String> = ids
            .query_map([], |row| row.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        drop(ids);
        for edition_id in edition_ids {
            if self.read_including_revoked(&edition_id).is_ok() {
                manifest_files_verified += 1;
            } else {
                projection_hashes_ok = false;
            }
        }
        let mut event_files_verified = 0_u64;
        for entry in fs::read_dir(self.root.join(".art/events")).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let mut event: serde_json::Value =
                serde_json::from_slice(&fs::read(path).map_err(io_error)?)
                    .map_err(internal_error)?;
            let stored_hash = event
                .as_object_mut()
                .and_then(|object| object.remove("event_hash"))
                .and_then(|value| value.as_str().map(str::to_owned));
            if stored_hash.as_deref() == Some(&canonical_json_hash(&event)) {
                event_files_verified += 1;
            } else {
                projection_hashes_ok = false;
            }
        }
        Ok(KnowledgeDiagnostics {
            integrity_ok: integrity == "ok",
            foreign_key_violations,
            journal_mode,
            proposal_count: count("SELECT COUNT(*) FROM knowledge_proposals")?,
            stale_proposal_count: count(
                "SELECT COUNT(*) FROM knowledge_proposals WHERE status='stale'",
            )?,
            pending_publish_intents: count(
                "SELECT COUNT(*) FROM publish_intents WHERE state!='committed'",
            )? + count(
                "SELECT COUNT(*) FROM event_intents WHERE state!='committed'",
            )?,
            projection_count: count("SELECT COUNT(*) FROM edition_projections")?,
            current_edition_count: count(
                "SELECT COUNT(*) FROM edition_projections WHERE current=1 AND revoked=0",
            )?,
            search_index_count: count("SELECT COUNT(*) FROM knowledge_fts")?,
            search_index_aligned: count("SELECT COUNT(*) FROM knowledge_fts")?
                == count("SELECT COUNT(*) FROM edition_projections")?,
            navigation_count: count("SELECT COUNT(*) FROM knowledge_navigation")?,
            navigation_aligned: self.navigation_aligned()?,
            manifest_files_verified,
            event_files_verified,
            projection_hashes_ok,
            wal_bytes: fs::metadata(self.control_db.with_extension("sqlite3-wal"))
                .map_or(0, |metadata| metadata.len()),
            control_file_mode: file_mode(&self.control_db)?,
        })
    }

    fn read_including_revoked(&self, id: &str) -> ArtResult<EditionRecord> {
        let connection = self.connection()?;
        let row: Option<EditionRow> = connection.query_row("SELECT knowledge_key,edition_number,title,markdown_path,manifest_path,markdown_sha256,manifest_sha256,published_at FROM edition_projections WHERE edition_id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?))).optional().map_err(db_error)?;
        let (
            knowledge_key,
            edition_number,
            title,
            markdown_path,
            manifest_path,
            markdown_sha256,
            manifest_sha256,
            published_at,
        ) = row.ok_or(ArtError::NotFound)?;
        let record = EditionRecord {
            edition_id: id.into(),
            knowledge_key,
            edition_number,
            title,
            markdown_path: self.resolve_projection_path(&markdown_path)?,
            manifest_path: self.resolve_projection_path(&manifest_path)?,
            markdown_sha256,
            manifest_sha256,
            published_at,
        };
        verify_record(&record)?;
        Ok(record)
    }

    fn portable_projection_path(&self, path: &Path) -> ArtResult<String> {
        ensure_target(&self.root, path)?;
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|error| ArtError::PathConflict(error.to_string()))?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ArtError::PathConflict(
                "invalid portable edition path".into(),
            ));
        }
        relative
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| ArtError::PathConflict("edition path is not UTF-8".into()))
    }

    fn resolve_projection_path(&self, stored: &str) -> ArtResult<PathBuf> {
        let stored_path = PathBuf::from(stored);
        let resolved = if stored_path.is_absolute() {
            stored_path
        } else {
            if stored_path.as_os_str().is_empty()
                || stored_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(ArtError::PathConflict(
                    "invalid portable edition path".into(),
                ));
            }
            self.root.join(stored_path)
        };
        ensure_target(&self.root, &resolved)?;
        Ok(resolved)
    }

    fn commit_event(
        &self,
        event_id: &str,
        edition_id: &str,
        kind: &str,
        event: &serde_json::Value,
    ) -> ArtResult<()> {
        let target = self
            .root
            .join(".art/events")
            .join(format!("{event_id}.{kind}.json"));
        let intent_id = format!("arti_{}", Ulid::new());
        self.connection()?.execute(
            "INSERT INTO event_intents(id,event_id,edition_id,target_path,state,created_at,updated_at) VALUES (?1,?2,?3,?4,'prepared',?5,?5)",
            params![intent_id,event_id,edition_id,target.to_string_lossy(),Utc::now().to_rfc3339()],
        ).map_err(db_error)?;
        atomic_create(
            &target,
            serde_json::to_string_pretty(event)
                .map_err(internal_error)?
                .as_bytes(),
        )?;
        self.reconcile_events()?;
        self.connection()?
            .execute(
                "UPDATE event_intents SET state='committed',updated_at=?2 WHERE id=?1",
                params![intent_id, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn list_all_projection_records(&self) -> ArtResult<Vec<EditionRecord>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT edition_id FROM edition_projections ORDER BY edition_id")
            .map_err(db_error)?;
        let ids: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        drop(statement);
        ids.into_iter()
            .map(|id| self.read_including_revoked(&id))
            .collect()
    }

    fn connection(&self) -> ArtResult<Connection> {
        let connection = Connection::open(&self.control_db).map_err(db_error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
            )
            .map_err(db_error)?;
        Ok(connection)
    }

    fn commitment(&self, source: &ProposalSourceLock) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.commitment_key);
        hasher.update(serde_json::to_vec(source).expect("source serializes"));
        hex::encode(hasher.finalize())
    }
}

fn validate_draft(draft: &KnowledgeDraft) -> ArtResult<()> {
    let key_ok = !draft.knowledge_key.is_empty()
        && draft.knowledge_key.len() <= 128
        && draft.knowledge_key.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'_'
                })
        });
    if !key_ok
        || draft.title.trim().is_empty()
        || draft.markdown.trim().is_empty()
        || draft.markdown.len() > 256 * 1024
    {
        return Err(ArtError::InvalidInput("invalid knowledge draft".into()));
    }
    let lower = draft.markdown.to_ascii_lowercase();
    if lower.contains("authorization: bearer")
        || lower.contains("begin openssh private key")
        || lower.contains("password=")
    {
        return Err(ArtError::InvalidInput(
            "knowledge draft contains secret-like material".into(),
        ));
    }
    Ok(())
}

fn ensure_target(root: &Path, target: &Path) -> ArtResult<()> {
    if !target.starts_with(root)
        || target
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(ArtError::PathConflict("edition path escapes vault".into()));
    }
    let mut current = root.to_path_buf();
    for component in target
        .strip_prefix(root)
        .map_err(|error| ArtError::PathConflict(error.to_string()))?
        .components()
    {
        current.push(component);
        if current.exists() {
            let metadata = fs::symlink_metadata(&current).map_err(io_error)?;
            if metadata.file_type().is_symlink() {
                return Err(ArtError::PathConflict(
                    "symbolic link in edition path".into(),
                ));
            }
        }
    }
    Ok(())
}

fn atomic_create(path: &Path, bytes: &[u8]) -> ArtResult<()> {
    if path.exists() {
        return Err(ArtError::PathConflict(
            "immutable target already exists".into(),
        ));
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|v| v.to_str()).unwrap_or("file")
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    fs::rename(&tmp, path).map_err(io_error)
}
fn verify_record(record: &EditionRecord) -> ArtResult<()> {
    let markdown = fs::read(&record.markdown_path).map_err(io_error)?;
    let manifest = fs::read(&record.manifest_path).map_err(io_error)?;
    if hex_digest(&markdown) != record.markdown_sha256
        || hex_digest(&manifest) != record.manifest_sha256
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(())
}
fn hex_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn search_document(markdown: &str) -> String {
    let normalized = markdown.nfkc().collect::<String>().to_lowercase();
    let cjk: Vec<char> = normalized.chars().filter(|value| is_cjk(*value)).collect();
    let bigrams = cjk
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{normalized}\n{bigrams}")
}

fn edition_section(markdown: &str, heading: &str) -> Option<String> {
    let marker = format!("## {heading}\n\n");
    let body = markdown.split_once(&marker)?.1;
    let section = body
        .split_once("\n\n## ")
        .map_or(body, |(section, _)| section)
        .trim();
    (!section.is_empty()).then(|| section.to_owned())
}

fn fts_expression(terms: &[String]) -> ArtResult<String> {
    let expression = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    if expression.is_empty() {
        return Err(ArtError::InvalidInput("search terms are required".into()));
    }
    Ok(expression)
}

const fn is_cjk(value: char) -> bool {
    matches!(value as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}
#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> ArtResult<()> {
    Ok(())
}
fn parse_status(value: &str) -> ArtResult<ProposalStatus> {
    match value {
        "draft" => Ok(ProposalStatus::Draft),
        "submitted" => Ok(ProposalStatus::Submitted),
        "under_review" => Ok(ProposalStatus::UnderReview),
        "changes_requested" => Ok(ProposalStatus::ChangesRequested),
        "approved" => Ok(ProposalStatus::Approved),
        "rejected" => Ok(ProposalStatus::Rejected),
        "stale" => Ok(ProposalStatus::Stale),
        "materialized" => Ok(ProposalStatus::Materialized),
        "archived" => Ok(ProposalStatus::Archived),
        _ => Err(ArtError::Internal("unknown proposal status".into())),
    }
}
#[allow(clippy::needless_pass_by_value)]
fn db_error(error: rusqlite::Error) -> ArtError {
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
#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> ArtError {
    ArtError::Io(error.to_string())
}
fn internal_error(error: impl std::fmt::Display) -> ArtError {
    ArtError::Internal(error.to_string())
}
#[cfg(unix)]
fn set_private_permissions(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}
#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(unix)]
fn file_mode(path: &Path) -> ArtResult<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    Ok(Some(
        fs::metadata(path).map_err(io_error)?.permissions().mode() & 0o777,
    ))
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> ArtResult<Option<u32>> {
    Ok(None)
}
