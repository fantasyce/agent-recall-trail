use std::{fs, path::Path, sync::Arc};

use art_agent_store::AgentVault;
use art_domain::{ArtResult, memory::MemoryStatus};
use art_knowledge::KnowledgeVault;
use chrono::Utc;

use crate::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, SemanticDocument, SemanticProjection,
    SemanticRank,
};

#[derive(Debug, Clone)]
pub struct SemanticRanks {
    pub private: Vec<SemanticRank>,
    pub knowledge: Vec<SemanticRank>,
}

#[derive(Debug, Clone)]
pub struct SemanticRuntime {
    provider: Arc<dyn EmbeddingProvider>,
    private: SemanticProjection,
    knowledge: SemanticProjection,
}

impl SemanticRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        endpoint: &EmbeddingEndpoint,
        provider: Arc<dyn EmbeddingProvider>,
        private_path: &Path,
        private_epoch: &str,
        knowledge_path: &Path,
        knowledge_epoch: &str,
    ) -> ArtResult<Self> {
        if provider.fingerprint() != endpoint.fingerprint() {
            return Err(art_domain::ArtError::InvalidInput(
                "semantic runtime provider fingerprint mismatch".into(),
            ));
        }
        Ok(Self {
            provider,
            private: SemanticProjection::open(private_path, endpoint, private_epoch)?,
            knowledge: SemanticProjection::open(knowledge_path, endpoint, knowledge_epoch)?,
        })
    }

    pub fn rank(
        &self,
        query: &str,
        private_limit: usize,
        knowledge_limit: usize,
    ) -> ArtResult<SemanticRanks> {
        let vectors = self.provider.embed(EmbeddingInput::Query(query))?;
        let query = vectors.first().ok_or(art_domain::ArtError::IndexDegraded)?;
        Ok(SemanticRanks {
            private: self.private.rank(query, private_limit)?,
            knowledge: self.knowledge.rank(query, knowledge_limit)?,
        })
    }
}

pub fn private_semantic_documents(vault: &AgentVault) -> ArtResult<Vec<SemanticDocument>> {
    let now = Utc::now();
    vault
        .list()?
        .into_iter()
        .filter(|memory| {
            memory.status == MemoryStatus::Active
                && memory.valid_from.is_none_or(|start| start <= now)
                && memory.valid_until.is_none_or(|end| end > now)
        })
        .map(|memory| {
            SemanticDocument::new(
                format!("memory:{}@{}", memory.id, memory.current_revision),
                format!(
                    "{}\n{}\n{}",
                    memory.title,
                    memory.summary,
                    serde_json::to_string(&memory.payload).unwrap_or_default()
                ),
                memory.current_hash,
            )
        })
        .collect()
}

pub fn knowledge_semantic_documents(vault: &KnowledgeVault) -> ArtResult<Vec<SemanticDocument>> {
    vault
        .list_current()?
        .into_iter()
        .map(|edition| {
            let text = fs::read_to_string(&edition.markdown_path)
                .map_err(|error| art_domain::ArtError::Io(error.to_string()))?;
            SemanticDocument::new(
                format!("knowledge:{}", edition.edition_id),
                text,
                edition.markdown_sha256,
            )
        })
        .collect()
}
