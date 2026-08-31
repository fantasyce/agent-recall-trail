//! Admission-gated Chinese lexical retrieval and Recall Bundles.

mod embedding;
mod navigation;
mod policy;
mod ranking;
mod semantic_projection;

pub use embedding::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, OpenAiCompatibleEmbeddingProvider,
    ProviderFingerprint,
};
pub use navigation::NavigationTopic;
pub use policy::{RecallDetail, RetrievalMode};
pub use semantic_projection::{
    SemanticDocument, SemanticProjection, SemanticRank, knowledge_semantic_path,
    private_semantic_path,
};

use std::{cmp::Ordering, collections::BTreeSet, fs, sync::OnceLock};

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
        }
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
        let terms = query.search_terms();
        let private_requested = request.max_private_results.unwrap_or(3);
        let knowledge_requested = request.max_knowledge_results.unwrap_or(3);
        let private_candidate_limit = (private_requested * 64).clamp(512, 2_048);
        let knowledge_candidate_limit = (knowledge_requested * 64).clamp(512, 2_048);
        let now = Utc::now();
        let mut memories = Vec::new();
        let mut cautions = Vec::new();
        let full_scan = request.mode == RetrievalMode::FullScan;
        let private_candidates = if full_scan {
            self.private_vault
                .list()?
                .into_iter()
                .enumerate()
                .map(|(index, artifact)| RankedMemoryCandidate {
                    artifact,
                    lexical_rank: index + 1,
                })
                .collect()
        } else {
            self.private_vault
                .search_ranked_candidates(&terms, private_candidate_limit)?
        };
        for candidate in private_candidates {
            let memory = candidate.artifact;
            let text = memory_text(&memory);
            let lexical = LexicalDocument::new(&text);
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
            if let Some(mut lexical_match) = lexical_match_indexed(&query, &lexical) {
                let status = format!("{:?}", memory.status).to_lowercase();
                let authority = if memory.status == MemoryStatus::Candidate {
                    0.75
                } else {
                    1.0
                };
                lexical_match.reasons.insert(
                    0,
                    if full_scan {
                        "canonical_full_scan"
                    } else {
                        "bm25_rank"
                    }
                    .into(),
                );
                memories.push(RecallItem {
                    subject_ref: format!("memory:{}@{}", memory.id, memory.current_revision),
                    title: memory.title.clone(),
                    excerpt: truncate(&memory.summary, 360),
                    origin: RecallOrigin::Memory,
                    status,
                    match_reasons: lexical_match.reasons,
                    score: rank_score(
                        candidate.lexical_rank,
                        lexical_match.exact,
                        lexical_match.token_coverage,
                        lexical_match.bigram_coverage,
                    ) * authority,
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
        let knowledge_candidates = if full_scan {
            self.knowledge_vault
                .list_current()?
                .into_iter()
                .enumerate()
                .map(|(index, edition)| RankedEditionCandidate {
                    edition,
                    lexical_rank: index + 1,
                })
                .collect()
        } else {
            self.knowledge_vault
                .search_ranked_candidates(&terms, knowledge_candidate_limit)?
        };
        for candidate in knowledge_candidates {
            let edition = candidate.edition;
            let text = knowledge_text(&edition)?;
            let lexical = LexicalDocument::new(&text);
            if let Some(mut lexical_match) = lexical_match_indexed(&query, &lexical) {
                lexical_match.reasons.insert(
                    0,
                    if full_scan {
                        "canonical_full_scan"
                    } else {
                        "bm25_rank"
                    }
                    .into(),
                );
                knowledge.push(RecallItem {
                    subject_ref: format!("knowledge:{}", edition.edition_id),
                    title: edition.title.clone(),
                    excerpt: truncate(&strip_frontmatter(&text), 480),
                    origin: RecallOrigin::Knowledge,
                    status: "published".into(),
                    match_reasons: lexical_match.reasons,
                    score: rank_score(
                        candidate.lexical_rank,
                        lexical_match.exact,
                        lexical_match.token_coverage,
                        lexical_match.bigram_coverage,
                    ) * 1.2,
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
            vector_status: "unavailable".into(),
            requested_mode: request.mode,
            effective_mode: request.mode,
            detail: request.detail,
            map_status: "unavailable".into(),
            candidate_sources: vec![if full_scan {
                "canonical_full_scan".into()
            } else {
                "lexical".into()
            }],
            fallback_reason: None,
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
