use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationTopic {
    pub lane: String,
    pub topic_key: String,
    pub title: String,
    pub count: usize,
    pub subject_refs: Vec<String>,
    pub match_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct NavigationCandidate {
    pub lane: String,
    pub topic_key: String,
    pub title: String,
    pub searchable_metadata: String,
    pub subject_ref: String,
    pub usage_count: u64,
}

#[derive(Debug)]
struct RankedTopic {
    topic: NavigationTopic,
    score: u64,
}

pub(crate) fn build_topics(
    query: &str,
    candidates: Vec<NavigationCandidate>,
) -> Vec<NavigationTopic> {
    let query = normalize(query);
    let query_tokens = tokens(&query);
    let query_bigrams = cjk_bigrams(&query);
    let mut grouped: BTreeMap<(String, String), RankedTopic> = BTreeMap::new();
    for candidate in candidates {
        let searchable = normalize(&candidate.searchable_metadata);
        let exact = searchable.contains(&query);
        let token_hits = query_tokens.intersection(&tokens(&searchable)).count();
        let bigram_hits = query_bigrams
            .intersection(&cjk_bigrams(&searchable))
            .count();
        if !exact && token_hits == 0 && bigram_hits == 0 {
            continue;
        }
        let mut reasons = Vec::new();
        if exact {
            reasons.push("metadata_exact".into());
        }
        if token_hits > 0 {
            reasons.push("metadata_token".into());
        }
        if bigram_hits > 0 {
            reasons.push("metadata_cjk_bigram".into());
        }
        let score = u64::from(exact) * 1_000_000
            + u64::try_from(token_hits).unwrap_or(u64::MAX) * 10_000
            + u64::try_from(bigram_hits).unwrap_or(u64::MAX) * 100
            + candidate.usage_count.min(99);
        let key = (candidate.lane.clone(), candidate.topic_key.clone());
        let ranked = grouped.entry(key).or_insert_with(|| RankedTopic {
            topic: NavigationTopic {
                lane: candidate.lane,
                topic_key: candidate.topic_key,
                title: candidate.title.clone(),
                count: 0,
                subject_refs: Vec::new(),
                match_reasons: reasons.clone(),
            },
            score,
        });
        ranked.topic.count += 1;
        if candidate.title < ranked.topic.title {
            ranked.topic.title = candidate.title;
        }
        if ranked.topic.subject_refs.len() < 8 {
            ranked.topic.subject_refs.push(candidate.subject_ref);
            ranked.topic.subject_refs.sort();
        }
        if score > ranked.score {
            ranked.score = score;
            ranked.topic.match_reasons = reasons;
        }
    }
    let mut ranked: Vec<_> = grouped.into_values().collect();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.topic.lane.cmp(&right.topic.lane))
            .then_with(|| left.topic.topic_key.cmp(&right.topic.topic_key))
    });
    ranked.truncate(12);
    ranked.into_iter().map(|item| item.topic).collect()
}

fn normalize(value: &str) -> String {
    value.nfkc().collect::<String>().to_lowercase()
}

fn tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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
