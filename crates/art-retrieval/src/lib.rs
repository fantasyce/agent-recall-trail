//! Admission-gated Chinese lexical retrieval and Recall Bundles.

mod ranking;

use std::{cmp::Ordering, collections::BTreeSet, fs, sync::OnceLock};

use art_agent_store::AgentVault;
use art_domain::{
    ArtError, ArtResult,
    memory::{MemoryArtifact, MemoryStatus},
};
use art_knowledge::{EditionRecord, KnowledgeVault};
use chrono::{DateTime, Duration, Utc};
use jieba_rs::{Jieba, TokenizeMode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;

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
    pub cautions: Vec<String>,
    pub omitted_private: usize,
    pub omitted_knowledge: usize,
    pub budget_tokens: usize,
    pub generated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub persist_policy: String,
    pub vector_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequest {
    pub query: String,
    pub include_candidates: bool,
    pub budget_tokens: usize,
    pub max_private_results: Option<usize>,
    pub max_knowledge_results: Option<usize>,
}

impl RecallRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
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
        let terms = query.search_terms();
        let private_requested = request.max_private_results.unwrap_or(3);
        let knowledge_requested = request.max_knowledge_results.unwrap_or(3);
        let private_candidate_limit = (private_requested * 64).clamp(512, 2_048);
        let knowledge_candidate_limit = (knowledge_requested * 64).clamp(512, 2_048);
        let now = Utc::now();
        let mut memories = Vec::new();
        let mut cautions = Vec::new();
        for candidate in self
            .private_vault
            .search_ranked_candidates(&terms, private_candidate_limit)?
        {
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
                lexical_match.reasons.insert(0, "bm25_rank".into());
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
        for candidate in self
            .knowledge_vault
            .search_ranked_candidates(&terms, knowledge_candidate_limit)?
        {
            let edition = candidate.edition;
            let text = knowledge_text(&edition)?;
            let lexical = LexicalDocument::new(&text);
            if let Some(mut lexical_match) = lexical_match_indexed(&query, &lexical) {
                lexical_match.reasons.insert(0, "bm25_rank".into());
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
            cautions,
            omitted_private,
            omitted_knowledge,
            budget_tokens: request.budget_tokens,
            generated_at,
            expires_at: generated_at + Duration::minutes(10),
            persist_policy: "no_automatic_capture".into(),
            vector_status: "unavailable".into(),
        })
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
