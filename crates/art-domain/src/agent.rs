//! Agent identity and path contracts.

use std::{
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ArtError, ArtResult};

const RESERVED: &[&str] = &["all", "shared", "system", "root", "admin", "knowledge"];

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AgentId").field(&self.0).finish()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for AgentId {
    type Err = ArtError;

    fn from_str(value: &str) -> ArtResult<Self> {
        let valid = Regex::new(r"^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$")
            .map_err(|error| ArtError::Internal(error.to_string()))?;
        if !valid.is_match(value) || value.contains("--") || RESERVED.contains(&value) {
            return Err(ArtError::InvalidInput(
                "invalid or reserved agent id".into(),
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Codex,
    Dsh,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentProfile {
    pub agent_id: AgentId,
    pub host: HostKind,
    pub schema_version: u32,
}

#[derive(Debug, Clone)]
pub struct ArtPaths {
    root: PathBuf,
}

impl ArtPaths {
    pub fn from_explicit_root(root: impl AsRef<Path>) -> ArtResult<Self> {
        let root = root.as_ref();
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(ArtError::PathConflict(
                "ART root must be an absolute normalized path".into(),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.root.join("data/art/agents")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("config/art/agents")
    }

    pub fn control_db(&self) -> PathBuf {
        self.root.join("data/art/control/art-control.sqlite3")
    }

    pub fn knowledge_vault(&self) -> PathBuf {
        self.root.join("data/art/knowledge-vault")
    }

    pub fn knowledge_index(&self) -> PathBuf {
        self.root.join("data/art/index/knowledge.sqlite3")
    }

    pub fn agent_vault(&self, agent: &AgentId) -> PathBuf {
        self.agents_dir().join(agent.as_str()).join("art.sqlite3")
    }

    pub fn agent_profile(&self, agent: &AgentId) -> PathBuf {
        self.profiles_dir().join(format!("{agent}.json"))
    }

    pub fn ensure_managed_path(&self, path: impl AsRef<Path>) -> ArtResult<PathBuf> {
        let path = path.as_ref();
        if path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
            || !path.starts_with(&self.root)
        {
            return Err(ArtError::PathConflict("path escapes ART root".into()));
        }
        Ok(path.to_path_buf())
    }
}
