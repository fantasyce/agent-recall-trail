use std::{collections::BTreeSet, fs, path::Path, sync::Arc};

use art_agent_store::AgentVault;
use art_domain::{ArtError, ArtResult, memory::MemoryStatus};
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
    private_epoch: String,
    knowledge_epoch: String,
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
            private_epoch: private_epoch.to_owned(),
            knowledge_epoch: knowledge_epoch.to_owned(),
        })
    }

    pub fn source_epochs_match(&self, private_epoch: &str, knowledge_epoch: &str) -> bool {
        self.private_epoch == private_epoch && self.knowledge_epoch == knowledge_epoch
    }

    pub fn rank(
        &self,
        query: &str,
        private_limit: usize,
        knowledge_limit: usize,
        private_admitted: &BTreeSet<String>,
        knowledge_admitted: &BTreeSet<String>,
    ) -> ArtResult<SemanticRanks> {
        let vectors = self.provider.embed(EmbeddingInput::Query(query))?;
        let query = vectors.first().ok_or(art_domain::ArtError::IndexDegraded)?;
        Ok(SemanticRanks {
            private: self
                .private
                .rank_admitted(query, private_limit, Some(private_admitted))?,
            knowledge: self.knowledge.rank_admitted(
                query,
                knowledge_limit,
                Some(knowledge_admitted),
            )?,
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

fn stable_semantic_snapshot<E, D>(
    mut current_epoch: E,
    collect_documents: D,
) -> ArtResult<(String, Vec<SemanticDocument>)>
where
    E: FnMut() -> ArtResult<String>,
    D: FnOnce() -> ArtResult<Vec<SemanticDocument>>,
{
    let epoch_before = current_epoch()?;
    let documents = collect_documents()?;
    let epoch_after = current_epoch()?;
    if epoch_before != epoch_after {
        return Err(ArtError::IndexDegraded);
    }
    Ok((epoch_before, documents))
}

pub fn private_semantic_snapshot(vault: &AgentVault) -> ArtResult<(String, Vec<SemanticDocument>)> {
    stable_semantic_snapshot(
        || vault.semantic_index_epoch(Utc::now()),
        || private_semantic_documents(vault),
    )
}

pub fn knowledge_semantic_snapshot(
    vault: &KnowledgeVault,
) -> ArtResult<(String, Vec<SemanticDocument>)> {
    stable_semantic_snapshot(
        || vault.index_epoch(),
        || knowledge_semantic_documents(vault),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use art_domain::ArtError;

    use super::stable_semantic_snapshot;

    #[test]
    fn semantic_snapshot_rejects_a_canonical_epoch_change_during_collection() {
        let calls = Cell::new(0);
        let result = stable_semantic_snapshot(
            || {
                let call = calls.get();
                calls.set(call + 1);
                Ok(if call == 0 { "before" } else { "after" }.to_owned())
            },
            || Ok(Vec::new()),
        );

        assert!(matches!(result, Err(ArtError::IndexDegraded)));
    }
}
