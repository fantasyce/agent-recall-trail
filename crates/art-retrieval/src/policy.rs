use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// User-selected retrieval strategy. Semantic modes remain optional and must
/// degrade to lexical retrieval when no healthy provider is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Lexical,
    FullScan,
    Semantic,
    Hybrid,
}

impl Default for RetrievalMode {
    fn default() -> Self {
        Self::Lexical
    }
}

/// Controls whether recall returns a compact route map or ranked recall items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecallDetail {
    Route,
    Recall,
}

impl Default for RecallDetail {
    fn default() -> Self {
        Self::Recall
    }
}
