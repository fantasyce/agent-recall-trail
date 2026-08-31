//! Admission-gated Chinese lexical retrieval and Recall Bundles.

mod embedding;
mod navigation;
mod policy;
mod ranking;
mod semantic;
mod semantic_projection;

pub use embedding::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, OpenAiCompatibleEmbeddingProvider,
    ProviderFingerprint,
};
pub use navigation::NavigationTopic;
pub use policy::{RecallDetail, RetrievalMode};
pub use semantic::{
    SemanticRanks, SemanticRuntime, knowledge_semantic_documents, private_semantic_documents,
};
pub use semantic_projection::{
    SemanticDocument, SemanticProjection, SemanticRank, knowledge_semantic_path,
    private_semantic_path,
};

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::OnceLock,
};

use art_agent_store::{AgentVault, RankedMemoryCandidate};
use art_domain::{
    ArtError, ArtResult,
    memory::{MemoryArtifact, MemoryScope, MemoryStatus},
};
use art_knowledge::{EditionRecord, KnowledgeVault, RankedEditionCandidate};
use chrono::{DateTime, Duration, Utc};
use jieba_rs::{Jieba, TokenizeMode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;

use crate::navigation::{NavigationCandidate, build_topics};
use crate::ranking::rank_score;

static JIEBA: OnceLock<Jieba> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallOrigin {
    Memory,
    Knowledge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallItem {
    pub subject_ref: String,
    pub title: String,
    pub excerpt: String,
    pub origin: RecallOrigin,
    pub status: String,
    pub match_reasons: Vec<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallBundle {
    pub schema: String,
    pub bundle_id: String,
    pub agent_id: String,
    pub query_hash: String,
    pub private_memories: Vec<RecallItem>,
    pub knowledge_editions: Vec<RecallItem>,
    pub navigation_topics: Vec<NavigationTopic>,
    pub cautions: Vec<String>,
    pub omitted_private: usize,
    pub omitted_knowledge: usize,
    pub budget_tokens: usize,
    pub generated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub persist_policy: String,
    pub vector_status: String,
    pub requested_mode: RetrievalMode,
    pub effective_mode: RetrievalMode,
    pub detail: RecallDetail,
    pub map_status: String,
    pub candidate_sources: Vec<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequest {
    pub query: String,
    #[serde(default)]
    pub mode: RetrievalMode,
    #[serde(default)]
    pub detail: RecallDetail,
    pub include_candidates: bool,
    pub budget_tokens: usize,
    pub max_private_results: Option<usize>,
    pub max_knowledge_results: Option<usize>,
}

impl RecallRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            mode: RetrievalMode::Lexical,
            detail: RecallDetail::Recall,
            include_candidates: false,
            budget_tokens: 1_800,
            max_private_results: None,
            max_knowledge_results: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecallEngine {
    private_vault: AgentVault,
    knowledge_vault: KnowledgeVault,
    semantic: Option<SemanticRuntime>,
}

#[derive(Debug)]
struct LexicalDocument {
    normalized: String,
    tokens: BTreeSet<String>,
    bigrams: BTreeSet<String>,
}

#[derive(Debug)]
struct LexicalQuery {
    normalized: String,
    tokens: BTreeSet<String>,
    bigrams: BTreeSet<String>,
}

#[derive(Debug)]
struct LexicalMatch {
    exact: bool,
    token_coverage: f64,
    bigram_coverage: f64,
    reasons: Vec<String>,
}

impl RecallEngine {
    pub fn new(private_vault: AgentVault, knowledge_vault: KnowledgeVault) -> Self {
        Self {
            private_vault,
            knowledge_vault,
            semantic: None,
        }
    }

    pub fn with_semantic(mut self, semantic: SemanticRuntime) -> Self {
        self.semantic = Some(semantic);
        self
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn recall(&self, request: RecallRequest) -> ArtResult<RecallBundle> {
        if request.query.trim().is_empty() || !(128..=6_000).contains(&request.budget_tokens) {
            return Err(ArtError::InvalidInput(
                "query is required and token budget must be 128..6000".into(),
            ));
        }
        if [request.max_private_results, request.max_knowledge_results]
            .into_iter()
            .flatten()
            .any(|depth| !(1..=20).contains(&depth))
        {
            return Err(ArtError::InvalidInput("result depth must be 1..=20".into()));
        }
        let query = LexicalQuery::new(&request.query);
        if request.detail == RecallDetail::Route {
            return self.route(&request);
        }
        let requested_mode = request.mode;
        let (mut effective_mode, mut vector_status, mut fallback_reason) = match request.mode {
            RetrievalMode::Semantic | RetrievalMode::Hybrid if self.semantic.is_none() => (
                RetrievalMode::Lexical,
                "unavailable",
                Some("semantic_unconfigured".to_owned()),
            ),
            RetrievalMode::Semantic | RetrievalMode::Hybrid => (request.mode, "ready", None),
            _ => (request.mode, "unavailable", None),
        };
        let terms = query.search_terms();
        let private_requested = request.max_private_results.unwrap_or(3);
        let knowledge_requested = request.max_knowledge_results.unwrap_or(3);
        let private_candidate_limit = (private_requested * 64).clamp(512, 2_048);
        let knowledge_candidate_limit = (knowledge_requested * 64).clamp(512, 2_048);
        let semantic_ranks = if matches!(
            effective_mode,
            RetrievalMode::Semantic | RetrievalMode::Hybrid
        ) {
            match self
                .semantic
                .as_ref()
                .expect("semantic runtime checked")
                .rank(
                    &request.query,
                    private_candidate_limit,
                    knowledge_candidate_limit,
                ) {
                Ok(ranks) => Some(ranks),
                Err(_) => {
                    effective_mode = RetrievalMode::Lexical;
                    vector_status = "degraded";
                    fallback_reason = Some("semantic_provider_failure".into());
                    None
                }
            }
        } else {
            None
        };
        let private_semantic: BTreeMap<_, _> = semantic_ranks
            .as_ref()
            .map(|ranks| {
                ranks
                    .private
                    .iter()
                    .map(|rank| (rank.subject_ref.clone(), rank.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let knowledge_semantic: BTreeMap<_, _> = semantic_ranks
            .as_ref()
            .map(|ranks| {
                ranks
                    .knowledge
                    .iter()
                    .map(|rank| (rank.subject_ref.clone(), rank.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let now = Utc::now();
        let mut memories = Vec::new();
        let mut cautions = Vec::new();
        let full_scan = effective_mode == RetrievalMode::FullScan;
        let mut private_candidates = if full_scan {
            self.private_vault
                .list()?
                .into_iter()
                .enumerate()
                .map(|(index, artifact)| RankedMemoryCandidate {
                    artifact,
                    lexical_rank: index + 1,
                })
                .collect()
        } else if effective_mode == RetrievalMode::Semantic {
            Vec::new()
        } else {
            self.private_vault
                .search_ranked_candidates(&terms, private_candidate_limit)?
        };
        let mut private_ids: BTreeSet<_> = private_candidates
            .iter()
            .map(|candidate| candidate.artifact.id.clone())
            .collect();
        for rank in private_semantic.values() {
            let Some((memory_id, revision)) = parse_memory_ref(&rank.subject_ref) else {
                continue;
            };
            let Ok(memory) = self.private_vault.read(memory_id) else {
                continue;
            };
            if memory.current_revision == revision && private_ids.insert(memory.id.clone()) {
                private_candidates.push(RankedMemoryCandidate {
                    artifact: memory,
                    lexical_rank: rank.rank,
                });
            }
        }
        for candidate in private_candidates {
            let memory = candidate.artifact;
            let text = memory_text(&memory);
            let lexical = LexicalDocument::new(&text);
            let subject_ref = format!("memory:{}@{}", memory.id, memory.current_revision);
            let semantic_rank = private_semantic.get(&subject_ref);
            if !eligible_memory(&memory, request.include_candidates, now) {
                if matches!(memory.status, MemoryStatus::Disputed)
                    && lexical_match_indexed(&query, &lexical).is_some()
                {
                    cautions.push(format!(
                        "disputed private memory exists for subject {}",
                        memory.id
                    ));
                }
                continue;
            }
            let lexical_match = lexical_match_indexed(&query, &lexical);
            let selected = match effective_mode {
                RetrievalMode::Semantic => semantic_rank.is_some(),
                RetrievalMode::Hybrid => lexical_match.is_some() || semantic_rank.is_some(),
                _ => lexical_match.is_some(),
            };
            if selected {
                let status = format!("{:?}", memory.status).to_lowercase();
                let authority = if memory.status == MemoryStatus::Candidate {
                    0.75
                } else {
                    1.0
                };
                let lexical_score = lexical_match.as_ref().map(|lexical_match| {
                    rank_score(
                        candidate.lexical_rank,
                        lexical_match.exact,
                        lexical_match.token_coverage,
                        lexical_match.bigram_coverage,
                    )
                });
                let mut reasons = lexical_match.map_or_else(Vec::new, |mut lexical_match| {
                    lexical_match.reasons.insert(
                        0,
                        if full_scan {
                            "canonical_full_scan"
                        } else {
                            "bm25_rank"
                        }
                        .into(),
                    );
                    lexical_match.reasons
                });
                if semantic_rank.is_some() {
                    reasons.push("semantic_rank".into());
                }
                let score = fused_score(effective_mode, lexical_score, semantic_rank) * authority;
                memories.push(RecallItem {
                    subject_ref,
                    title: memory.title.clone(),
                    excerpt: truncate(&memory.summary, 360),
                    origin: RecallOrigin::Memory,
                    status,
                    match_reasons: reasons,
                    score,
                });
                if unsafe_text(&text) {
                    cautions.push(format!(
                        "unsafe instruction-like text flagged in memory {}",
                        memory.id
                    ));
                }
            }
        }
        let mut knowledge = Vec::new();
        let mut knowledge_candidates = if full_scan {
            self.knowledge_vault
                .list_current()?
                .into_iter()
                .enumerate()
                .map(|(index, edition)| RankedEditionCandidate {
                    edition,
                    lexical_rank: index + 1,
                })
                .collect()
        } else if effective_mode == RetrievalMode::Semantic {
            Vec::new()
        } else {
            self.knowledge_vault
                .search_ranked_candidates(&terms, knowledge_candidate_limit)?
        };
        let mut knowledge_ids: BTreeSet<_> = knowledge_candidates
            .iter()
            .map(|candidate| candidate.edition.edition_id.clone())
            .collect();
        for rank in knowledge_semantic.values() {
            let Some(edition_id) = rank.subject_ref.strip_prefix("knowledge:") else {
                continue;
            };
            let Ok(edition) = self.knowledge_vault.read(edition_id) else {
                continue;
            };
            if knowledge_ids.insert(edition.edition_id.clone()) {
                knowledge_candidates.push(RankedEditionCandidate {
                    edition,
                    lexical_rank: rank.rank,
                });
            }
        }
        for candidate in knowledge_candidates {
            let edition = candidate.edition;
            let text = knowledge_text(&edition)?;
            let lexical = LexicalDocument::new(&text);
            let subject_ref = format!("knowledge:{}", edition.edition_id);
            let semantic_rank = knowledge_semantic.get(&subject_ref);
            let lexical_match = lexical_match_indexed(&query, &lexical);
            let selected = match effective_mode {
                RetrievalMode::Semantic => semantic_rank.is_some(),
                RetrievalMode::Hybrid => lexical_match.is_some() || semantic_rank.is_some(),
                _ => lexical_match.is_some(),
            };
            if selected {
                let lexical_score = lexical_match.as_ref().map(|lexical_match| {
                    rank_score(
                        candidate.lexical_rank,
                        lexical_match.exact,
                        lexical_match.token_coverage,
                        lexical_match.bigram_coverage,
                    )
                });
                let mut reasons = lexical_match.map_or_else(Vec::new, |mut lexical_match| {
                    lexical_match.reasons.insert(
                        0,
                        if full_scan {
                            "canonical_full_scan"
                        } else {
                            "bm25_rank"
                        }
                        .into(),
                    );
                    lexical_match.reasons
                });
                if semantic_rank.is_some() {
                    reasons.push("semantic_rank".into());
                }
                knowledge.push(RecallItem {
                    subject_ref,
                    title: edition.title.clone(),
                    excerpt: truncate(&strip_frontmatter(&text), 480),
                    origin: RecallOrigin::Knowledge,
                    status: "published".into(),
                    match_reasons: reasons,
                    score: fused_score(effective_mode, lexical_score, semantic_rank) * 1.2,
                });
                if unsafe_text(&text) {
                    cautions.push(format!(
                        "unsafe instruction-like text flagged in knowledge {}",
                        edition.edition_id
                    ));
                }
            }
        }
        sort_items(&mut memories);
        sort_items(&mut knowledge);
        let private_cap = private_requested.min((request.budget_tokens * 35 / 100 / 120).max(1));
        let knowledge_cap =
            knowledge_requested.min((request.budget_tokens * 55 / 100 / 160).max(1));
        let omitted_private = memories.len().saturating_sub(private_cap);
        let omitted_knowledge = knowledge.len().saturating_sub(knowledge_cap);
        memories.truncate(private_cap);
        knowledge.truncate(knowledge_cap);
        let generated_at = Utc::now();
        Ok(RecallBundle {
            schema: "art.recall.v1".into(),
            bundle_id: format!("artb_{}", Ulid::new()),
            agent_id: self.private_vault.agent_id().to_string(),
            query_hash: hex::encode(Sha256::digest(request.query.as_bytes())),
            private_memories: memories,
            knowledge_editions: knowledge,
            navigation_topics: Vec::new(),
            cautions,
            omitted_private,
            omitted_knowledge,
            budget_tokens: request.budget_tokens,
            generated_at,
            expires_at: generated_at + Duration::minutes(10),
            persist_policy: "no_automatic_capture".into(),
            vector_status: vector_status.into(),
            requested_mode,
            effective_mode,
            detail: request.detail,
            map_status: "unavailable".into(),
            candidate_sources: match effective_mode {
                RetrievalMode::FullScan => vec!["canonical_full_scan".into()],
                RetrievalMode::Semantic => vec!["semantic".into()],
                RetrievalMode::Hybrid => vec!["lexical".into(), "semantic".into()],
                RetrievalMode::Lexical => vec!["lexical".into()],
            },
            fallback_reason,
        })
    }

    fn route(&self, request: &RecallRequest) -> ArtResult<RecallBundle> {
        let mut map_status = "ready".to_owned();
        let private_entries = match self.private_vault.navigation_aligned() {
            Ok(true) => self.private_vault.navigation_entries(),
            Ok(false) | Err(_) => self
                .private_vault
                .rebuild_navigation()
                .and_then(|_| self.private_vault.navigation_entries()),
        };
        let knowledge_entries = match self.knowledge_vault.navigation_aligned() {
            Ok(true) => self.knowledge_vault.navigation_entries(),
            Ok(false) | Err(_) => self
                .knowledge_vault
                .rebuild_navigation()
                .and_then(|_| self.knowledge_vault.navigation_entries()),
        };
        let mut candidates = Vec::new();
        match private_entries {
            Ok(entries) => {
                candidates.extend(entries.into_iter().map(|entry| NavigationCandidate {
                    lane: "private_memory".into(),
                    topic_key: format!("{}:{}", entry.scope_type, entry.scope_key),
                    searchable_metadata: format!(
                        "{} {} {} {}",
                        entry.title, entry.kind, entry.scope_type, entry.scope_key
                    ),
                    subject_ref: format!("memory:{}@{}", entry.memory_id, entry.revision),
                    title: entry.title,
                    usage_count: entry.usage_count,
                }))
            }
            Err(_) => {
                map_status = "degraded".into();
                candidates.extend(self.canonical_private_navigation_candidates()?);
            }
        }
        match knowledge_entries {
            Ok(entries) => {
                candidates.extend(entries.into_iter().map(|entry| NavigationCandidate {
                    lane: "shared_knowledge".into(),
                    topic_key: entry.knowledge_key.clone(),
                    searchable_metadata: format!(
                        "{} {} {}",
                        entry.title, entry.knowledge_key, entry.applicability
                    ),
                    subject_ref: format!("knowledge:{}", entry.edition_id),
                    title: entry.title,
                    usage_count: entry.usage_count,
                }))
            }
            Err(_) => {
                map_status = "degraded".into();
                candidates.extend(self.canonical_knowledge_navigation_candidates()?);
            }
        }
        let generated_at = Utc::now();
        Ok(RecallBundle {
            schema: "art.recall.v1".into(),
            bundle_id: format!("artb_{}", Ulid::new()),
            agent_id: self.private_vault.agent_id().to_string(),
            query_hash: hex::encode(Sha256::digest(request.query.as_bytes())),
            private_memories: Vec::new(),
            knowledge_editions: Vec::new(),
            navigation_topics: build_topics(&request.query, candidates),
            cautions: Vec::new(),
            omitted_private: 0,
            omitted_knowledge: 0,
            budget_tokens: request.budget_tokens,
            generated_at,
            expires_at: generated_at + Duration::minutes(10),
            persist_policy: "no_automatic_capture".into(),
            vector_status: "unavailable".into(),
            requested_mode: request.mode,
            effective_mode: request.mode,
            detail: RecallDetail::Route,
            map_status,
            candidate_sources: vec!["private_navigation".into(), "shared_navigation".into()],
            fallback_reason: None,
        })
    }

    fn canonical_private_navigation_candidates(&self) -> ArtResult<Vec<NavigationCandidate>> {
        let now = Utc::now();
        Ok(self
            .private_vault
            .list()?
            .into_iter()
            .filter(|memory| eligible_memory(memory, false, now))
            .map(|memory| {
                let (scope_type, scope_key) = navigation_scope(&memory.scope);
                NavigationCandidate {
                    lane: "private_memory".into(),
                    topic_key: format!("{scope_type}:{scope_key}"),
                    searchable_metadata: format!(
                        "{} {} {scope_type} {scope_key}",
                        memory.title,
                        memory.payload.kind_name()
                    ),
                    subject_ref: format!("memory:{}@{}", memory.id, memory.current_revision),
                    title: memory.title,
                    usage_count: 0,
                }
            })
            .collect())
    }

    fn canonical_knowledge_navigation_candidates(&self) -> ArtResult<Vec<NavigationCandidate>> {
        Ok(self
            .knowledge_vault
            .list_current()?
            .into_iter()
            .map(|edition| NavigationCandidate {
                lane: "shared_knowledge".into(),
                topic_key: edition.knowledge_key.clone(),
                searchable_metadata: format!("{} {}", edition.title, edition.knowledge_key),
                subject_ref: format!("knowledge:{}", edition.edition_id),
                title: edition.title,
                usage_count: 0,
            })
            .collect())
    }
}

fn navigation_scope(scope: &MemoryScope) -> (&'static str, &str) {
    match scope {
        MemoryScope::Session(key) => ("session", key),
        MemoryScope::Repository(key) => ("repository", key),
        MemoryScope::Workspace(key) => ("workspace", key),
        MemoryScope::Machine(key) => ("machine", key),
        MemoryScope::User(key) => ("user", key),
    }
}

fn parse_memory_ref(subject_ref: &str) -> Option<(&str, u32)> {
    let (memory_id, revision) = subject_ref.strip_prefix("memory:")?.split_once('@')?;
    Some((memory_id, revision.parse().ok()?))
}

fn fused_score(mode: RetrievalMode, lexical: Option<f64>, semantic: Option<&SemanticRank>) -> f64 {
    let (semantic_quality, semantic_rrf) = semantic.map_or((0.0, 0.0), |rank| {
        let similarity = (f64::from(rank.cosine_similarity).clamp(-1.0, 1.0) + 1.0) / 2.0;
        let reciprocal_rank =
            1.0 / (60.0 + f64::from(u32::try_from(rank.rank).unwrap_or(u32::MAX)));
        (similarity + reciprocal_rank, reciprocal_rank)
    });
    match mode {
        RetrievalMode::Semantic => semantic_quality,
        RetrievalMode::Hybrid => lexical.unwrap_or_default() + semantic_rrf * 0.7,
        RetrievalMode::Lexical | RetrievalMode::FullScan => lexical.unwrap_or_default(),
    }
}

fn eligible_memory(memory: &MemoryArtifact, include_candidates: bool, now: DateTime<Utc>) -> bool {
    let status_ok = memory.status == MemoryStatus::Active
        || (include_candidates && memory.status == MemoryStatus::Candidate);
    status_ok
        && memory.valid_from.is_none_or(|start| start <= now)
        && memory.valid_until.is_none_or(|end| end > now)
}

fn memory_text(memory: &MemoryArtifact) -> String {
    format!(
        "{}\n{}\n{}",
        memory.title,
        memory.summary,
        serde_json::to_string(&memory.payload).unwrap_or_default()
    )
}

fn knowledge_text(edition: &EditionRecord) -> ArtResult<String> {
    fs::read_to_string(&edition.markdown_path).map_err(|error| ArtError::Io(error.to_string()))
}

fn normalize(value: &str) -> String {
    value.nfkc().collect::<String>().to_lowercase()
}

impl LexicalQuery {
    fn new(value: &str) -> Self {
        let normalized = normalize(value);
        Self {
            tokens: tokenize(&normalized),
            bigrams: cjk_bigrams(&normalized),
            normalized,
        }
    }

    fn search_terms(&self) -> Vec<String> {
        let mut tail = self.tokens.iter().cloned().collect::<BTreeSet<_>>();
        tail.extend(self.bigrams.iter().cloned());
        tail.remove(&self.normalized);
        std::iter::once(self.normalized.clone())
            .chain(tail)
            .filter(|term| !term.trim().is_empty())
            .collect()
    }
}

impl LexicalDocument {
    fn new(value: &str) -> Self {
        let normalized = normalize(value);
        Self {
            tokens: tokenize(&normalized),
            bigrams: cjk_bigrams(&normalized),
            normalized,
        }
    }
}

fn tokenize(value: &str) -> BTreeSet<String> {
    JIEBA
        .get_or_init(Jieba::new)
        .tokenize(value, TokenizeMode::Search, false)
        .into_iter()
        .map(|token| token.word.to_owned())
        .filter(|token| !token.trim().is_empty())
        .collect()
}

fn lexical_match_indexed(query: &LexicalQuery, document: &LexicalDocument) -> Option<LexicalMatch> {
    let mut reasons = Vec::new();
    let exact = document.normalized.contains(&query.normalized);
    if exact {
        reasons.push("exact_or_substring".into());
    }
    let token_hits = query.tokens.intersection(&document.tokens).count();
    let token_coverage = ratio(token_hits, query.tokens.len());
    if token_hits > 0 {
        reasons.push("jieba_token".into());
    }
    let bigram_hits = query.bigrams.intersection(&document.bigrams).count();
    let bigram_coverage = ratio(bigram_hits, query.bigrams.len());
    if bigram_hits > 0 {
        reasons.push("cjk_bigram".into());
    }
    (exact || token_hits > 0 || bigram_hits > 0).then_some(LexicalMatch {
        exact,
        token_coverage,
        bigram_coverage,
        reasons,
    })
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    let numerator = u32::try_from(numerator).unwrap_or(u32::MAX);
    let denominator = u32::try_from(denominator.max(1)).unwrap_or(u32::MAX);
    f64::from(numerator) / f64::from(denominator)
}

fn cjk_bigrams(value: &str) -> BTreeSet<String> {
    let chars: Vec<_> = value
        .chars()
        .filter(|character| is_cjk(*character))
        .collect();
    chars.windows(2).map(|pair| pair.iter().collect()).collect()
}

const fn is_cjk(value: char) -> bool {
    matches!(value as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn unsafe_text(value: &str) -> bool {
    let lower = value.to_lowercase();
    [
        "ignore previous",
        "忽略之前",
        "泄露配置",
        "authorization: bearer",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn strip_frontmatter(value: &str) -> String {
    value
        .split_once("---\n\n")
        .map_or_else(|| value.to_owned(), |(_, body)| body.to_owned())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}

fn sort_items(items: &mut [RecallItem]) {
    items.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.subject_ref.cmp(&right.subject_ref))
    });
}
