use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type ArtResult<T> = Result<T, ArtError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", content = "message")]
pub enum ArtError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found")]
    NotFound,
    #[error("identity mismatch")]
    IdentityMismatch,
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("source required")]
    SourceRequired,
    #[error("source is stale")]
    SourceStale,
    #[error("access grant is required")]
    GrantRequired,
    #[error("access grant is expired or exhausted")]
    GrantExpired,
    #[error("content marked no-persist")]
    NoPersist,
    #[error("invalid state transition")]
    InvalidStateTransition,
    #[error("duplicate idempotency key conflicts with existing payload")]
    DuplicateConflict,
    #[error("path conflict: {0}")]
    PathConflict(String),
    #[error("database schema is newer than this binary")]
    SchemaTooNew,
    #[error("database busy")]
    DbBusy,
    #[error("knowledge index degraded")]
    IndexDegraded,
    #[error("edition is not committed")]
    EditionNotCommitted,
    #[error("I/O error: {0}")]
    Io(String),
    #[error("service is shutting down")]
    ShuttingDown,
    #[error("internal error: {0}")]
    Internal(String),
}

impl ArtError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "ART_INVALID_INPUT",
            Self::NotFound => "ART_NOT_FOUND",
            Self::IdentityMismatch => "ART_IDENTITY_MISMATCH",
            Self::PermissionDenied(_) => "ART_PERMISSION_DENIED",
            Self::SourceRequired => "ART_SOURCE_REQUIRED",
            Self::SourceStale => "ART_SOURCE_STALE",
            Self::GrantRequired => "ART_GRANT_REQUIRED",
            Self::GrantExpired => "ART_GRANT_EXPIRED",
            Self::NoPersist => "ART_NO_PERSIST",
            Self::InvalidStateTransition => "ART_INVALID_STATE_TRANSITION",
            Self::DuplicateConflict => "ART_DUPLICATE_CONFLICT",
            Self::PathConflict(_) => "ART_PATH_CONFLICT",
            Self::SchemaTooNew => "ART_SCHEMA_TOO_NEW",
            Self::DbBusy => "ART_DB_BUSY",
            Self::IndexDegraded => "ART_INDEX_DEGRADED",
            Self::EditionNotCommitted => "ART_EDITION_NOT_COMMITTED",
            Self::Io(_) => "ART_IO_ERROR",
            Self::ShuttingDown => "ART_SHUTTING_DOWN",
            Self::Internal(_) => "ART_INTERNAL",
        }
    }

    pub const fn retryable(&self) -> bool {
        matches!(
            self,
            Self::DbBusy | Self::IndexDegraded | Self::ShuttingDown
        )
    }
}
