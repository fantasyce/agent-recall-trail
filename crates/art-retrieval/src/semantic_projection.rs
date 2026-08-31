use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use art_domain::{ArtError, ArtResult};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::{EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider};

const SCHEMA_VERSION: &str = "1";
const BATCH_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDocument {
    pub subject_ref: String,
    pub text: String,
    pub content_sha256: String,
}

impl SemanticDocument {
    pub fn new(
        subject_ref: impl Into<String>,
        text: impl Into<String>,
        content_sha256: impl Into<String>,
    ) -> ArtResult<Self> {
        let document = Self {
            subject_ref: subject_ref.into(),
            text: text.into(),
            content_sha256: content_sha256.into(),
        };
        if document.subject_ref.trim().is_empty()
            || document.text.trim().is_empty()
            || document.content_sha256.len() != 64
            || !document
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ArtError::InvalidInput(
                "semantic document requires subject, text, and SHA-256".into(),
            ));
        }
        Ok(document)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticRank {
    pub subject_ref: String,
    pub rank: usize,
    pub cosine_similarity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticRebuildProgress {
    pub completed: u64,
    pub total: u64,
    pub resumed: bool,
}

#[derive(Debug, Clone)]
pub struct SemanticProjection {
    path: PathBuf,
    dimensions: usize,
}

pub fn private_semantic_path(agent_vault: &Path) -> PathBuf {
    agent_vault
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("retrieval/semantic.sqlite3")
}

pub fn knowledge_semantic_path(knowledge_root: &Path) -> PathBuf {
    knowledge_root.join(".art/retrieval/semantic.sqlite3")
}

impl SemanticProjection {
    pub fn rebuild(
        path: &Path,
        endpoint: &EmbeddingEndpoint,
        source_epoch: &str,
        documents: &[SemanticDocument],
        provider: &dyn EmbeddingProvider,
    ) -> ArtResult<u64> {
        Self::rebuild_with_progress(path, endpoint, source_epoch, documents, provider, &|_| {})
    }

    pub fn rebuild_with_progress(
        path: &Path,
        endpoint: &EmbeddingEndpoint,
        source_epoch: &str,
        documents: &[SemanticDocument],
        provider: &dyn EmbeddingProvider,
        progress: &dyn Fn(SemanticRebuildProgress),
    ) -> ArtResult<u64> {
        if source_epoch.trim().is_empty() || provider.fingerprint() != endpoint.fingerprint() {
            return Err(ArtError::InvalidInput(
                "semantic projection provider or source epoch mismatch".into(),
            ));
        }
        let mut unique = BTreeSet::new();
        if documents
            .iter()
            .any(|document| !unique.insert(document.subject_ref.as_str()))
        {
            return Err(ArtError::DuplicateConflict);
        }
        let parent = path.parent().ok_or_else(|| {
            ArtError::InvalidInput("semantic projection requires a parent".into())
        })?;
        fs::create_dir_all(parent).map_err(safe_io)?;
        set_private_directory(parent)?;
        let staging = path.with_extension("sqlite3.staging");
        let build_fingerprint = build_fingerprint(endpoint, source_epoch, documents);
        let expected: BTreeMap<_, _> = documents
            .iter()
            .map(|document| {
                (
                    document.subject_ref.as_str(),
                    document.content_sha256.as_str(),
                )
            })
            .collect();
        let mut connection = if staging.exists() {
            match open_staging(&staging, &build_fingerprint, &expected) {
                Ok(connection) => connection,
                Err(_) => {
                    fs::remove_file(&staging).map_err(safe_io)?;
                    create_staging(
                        &staging,
                        endpoint,
                        source_epoch,
                        &build_fingerprint,
                        documents.len(),
                    )?
                }
            }
        } else {
            create_staging(
                &staging,
                endpoint,
                source_epoch,
                &build_fingerprint,
                documents.len(),
            )?
        };
        let completed = completed_subjects(&connection)?;
        let resumed = !completed.is_empty();
        let mut completed_count = completed.len();
        progress(SemanticRebuildProgress {
            completed: u64::try_from(completed_count)
                .map_err(|error| ArtError::Internal(error.to_string()))?,
            total: u64::try_from(documents.len())
                .map_err(|error| ArtError::Internal(error.to_string()))?,
            resumed,
        });
        let pending: Vec<_> = documents
            .iter()
            .filter(|document| !completed.contains(&document.subject_ref))
            .collect();
        for batch in pending.chunks(BATCH_SIZE) {
            let texts: Vec<String> = batch
                .iter()
                .map(|document| document.text.chars().take(16_384).collect())
                .collect();
            let vectors = provider.embed(EmbeddingInput::Documents(&texts))?;
            if vectors.len() != batch.len() {
                return Err(ArtError::IndexDegraded);
            }
            let transaction = connection.transaction().map_err(db_error)?;
            for (document, vector) in batch.iter().zip(vectors) {
                let vector = normalize_checked(vector, endpoint.dimensions())?;
                transaction
                    .execute(
                        "INSERT INTO vectors(subject_ref,content_sha256,vector) VALUES (?1,?2,?3)",
                        params![
                            document.subject_ref,
                            document.content_sha256,
                            encode_vector(&vector)
                        ],
                    )
                    .map_err(db_error)?;
            }
            transaction.commit().map_err(db_error)?;
            completed_count += batch.len();
            progress(SemanticRebuildProgress {
                completed: u64::try_from(completed_count)
                    .map_err(|error| ArtError::Internal(error.to_string()))?,
                total: u64::try_from(documents.len())
                    .map_err(|error| ArtError::Internal(error.to_string()))?,
                resumed,
            });
        }
        connection
            .execute(
                "UPDATE metadata SET value='complete' WHERE key='build_status'",
                [],
            )
            .map_err(db_error)?;
        connection
            .execute_batch("PRAGMA optimize")
            .map_err(db_error)?;
        drop(connection);
        fs::rename(&staging, path).map_err(safe_io)?;
        set_private_file(path)?;
        u64::try_from(documents.len()).map_err(|error| ArtError::Internal(error.to_string()))
    }

    pub fn open(
        path: &Path,
        endpoint: &EmbeddingEndpoint,
        expected_epoch: &str,
    ) -> ArtResult<Self> {
        validate_private_projection(path)?;
        let connection = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        let value = |key: &str| -> ArtResult<String> {
            connection
                .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                    row.get(0)
                })
                .map_err(db_error)
        };
        let fingerprint = endpoint.fingerprint();
        if value("schema_version")? != SCHEMA_VERSION
            || value("provider_fingerprint")? != fingerprint.value
            || value("dimensions")? != endpoint.dimensions().to_string()
            || value("source_epoch")? != expected_epoch
            || value("build_status")? != "complete"
        {
            return Err(ArtError::IndexDegraded);
        }
        Ok(Self {
            path: path.to_owned(),
            dimensions: endpoint.dimensions(),
        })
    }

    pub fn rank(&self, query: &[f32], limit: usize) -> ArtResult<Vec<SemanticRank>> {
        if !(1..=2_048).contains(&limit) {
            return Err(ArtError::InvalidInput(
                "semantic rank limit must be 1..=2048".into(),
            ));
        }
        let query = normalize_checked(query.to_vec(), self.dimensions)?;
        let connection = Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        let mut statement = connection
            .prepare("SELECT subject_ref,vector FROM vectors ORDER BY subject_ref")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(db_error)?;
        let mut scored = Vec::new();
        for row in rows {
            let (subject_ref, bytes) = row.map_err(db_error)?;
            let vector = decode_vector(&bytes, self.dimensions)?;
            let score: f32 = query
                .iter()
                .zip(vector)
                .map(|(left, right)| left * right)
                .sum();
            scored.push((subject_ref, score));
        }
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        scored.truncate(limit);
        Ok(scored
            .into_iter()
            .enumerate()
            .map(|(index, (subject_ref, cosine_similarity))| SemanticRank {
                subject_ref,
                rank: index + 1,
                cosine_similarity,
            })
            .collect())
    }
}

fn create_staging(
    path: &Path,
    endpoint: &EmbeddingEndpoint,
    source_epoch: &str,
    build_fingerprint: &str,
    total: usize,
) -> ArtResult<Connection> {
    let mut connection = Connection::open(path).map_err(db_error)?;
    set_private_file(path)?;
    connection.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
         CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
         CREATE TABLE vectors(subject_ref TEXT PRIMARY KEY,content_sha256 TEXT NOT NULL,vector BLOB NOT NULL);",
    ).map_err(db_error)?;
    let transaction = connection.transaction().map_err(db_error)?;
    let fingerprint = endpoint.fingerprint();
    for (key, value) in [
        ("schema_version", SCHEMA_VERSION.to_owned()),
        ("provider_fingerprint", fingerprint.value),
        ("dimensions", endpoint.dimensions().to_string()),
        ("source_epoch", source_epoch.to_owned()),
        ("build_fingerprint", build_fingerprint.to_owned()),
        ("total_documents", total.to_string()),
        ("build_status", "in_progress".to_owned()),
    ] {
        transaction
            .execute(
                "INSERT INTO metadata(key,value) VALUES (?1,?2)",
                params![key, value],
            )
            .map_err(db_error)?;
    }
    transaction.commit().map_err(db_error)?;
    Ok(connection)
}

fn open_staging(
    path: &Path,
    build_fingerprint: &str,
    expected: &BTreeMap<&str, &str>,
) -> ArtResult<Connection> {
    validate_private_projection(path)?;
    let connection = Connection::open(path).map_err(db_error)?;
    let value = |key: &str| -> ArtResult<String> {
        connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .map_err(db_error)
    };
    if value("build_fingerprint")? != build_fingerprint || value("build_status")? != "in_progress" {
        return Err(ArtError::IndexDegraded);
    }
    let mut statement = connection
        .prepare("SELECT subject_ref,content_sha256 FROM vectors")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    for row in rows {
        let (subject, hash) = row.map_err(db_error)?;
        if expected.get(subject.as_str()).copied() != Some(hash.as_str()) {
            return Err(ArtError::IndexDegraded);
        }
    }
    drop(statement);
    Ok(connection)
}

fn completed_subjects(connection: &Connection) -> ArtResult<BTreeSet<String>> {
    let mut statement = connection
        .prepare("SELECT subject_ref FROM vectors")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

fn build_fingerprint(
    endpoint: &EmbeddingEndpoint,
    epoch: &str,
    documents: &[SemanticDocument],
) -> String {
    let mut rows: Vec<_> = documents
        .iter()
        .map(|document| (&document.subject_ref, &document.content_sha256))
        .collect();
    rows.sort();
    let mut digest = Sha256::new();
    digest.update(endpoint.fingerprint().value.as_bytes());
    digest.update(epoch.as_bytes());
    for (subject, hash) in rows {
        digest.update(subject.as_bytes());
        digest.update([0]);
        digest.update(hash.as_bytes());
    }
    hex::encode(digest.finalize())
}

fn normalize_checked(mut vector: Vec<f32>, dimensions: usize) -> ArtResult<Vec<f32>> {
    if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
        return Err(ArtError::IndexDegraded);
    }
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err(ArtError::IndexDegraded);
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok(vector)
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_vector(bytes: &[u8], dimensions: usize) -> ArtResult<Vec<f32>> {
    if bytes.len() != dimensions * 4 {
        return Err(ArtError::IndexDegraded);
    }
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(ArtError::IndexDegraded);
    }
    chunks
        .iter()
        .map(|chunk| {
            let value = f32::from_le_bytes(*chunk);
            value
                .is_finite()
                .then_some(value)
                .ok_or(ArtError::IndexDegraded)
        })
        .collect()
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(safe_io)
}
#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(safe_io)
}
#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_private_projection(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path).map_err(safe_io)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(())
}
#[cfg(not(unix))]
fn validate_private_projection(path: &Path) -> ArtResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(safe_io)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ArtError::IndexDegraded);
    }
    Ok(())
}

fn safe_io(_error: std::io::Error) -> ArtError {
    ArtError::Io("semantic projection file access failed".into())
}
fn db_error(_error: rusqlite::Error) -> ArtError {
    ArtError::IndexDegraded
}
